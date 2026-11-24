//! Region-local random ticking. Sampling follows ServerLevel.tickChunk; empty
//! sections are skipped as in Lithium's chunk-section random-tick counters.
use super::{
    anvil::AnvilChunkSource,
    fluid::RegionFluidWorld,
    region::{Signal, Tracking},
};
use ferrite_entity::Rng;
use ferrite_fluid::FluidWorld;
use ferrite_region::ChunkPos;
use ferrite_world::Chunk;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc::UnboundedSender;

type Pos = (i32, i32, i32);
type SaplingCell = ([i32; 3], u32);
type TwoByTwoFootprint = ([i32; 3], Vec<SaplingCell>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TreeGrowthRequest {
    pub origin: [i32; 3],
    pub saplings: Vec<([i32; 3], u32)>,
    pub feature: &'static str,
}

#[derive(Debug)]
pub(super) struct ValidatedTree {
    pub blocks: Vec<([i32; 3], u32, u16)>,
    pub entities: Vec<([i32; 3], ferrite_nbt::Compound)>,
}

trait RandomWorld {
    fn block(&self, p: Pos) -> u32;
    fn set_block(&mut self, p: Pos, state: u32);
    fn brightness(&self, p: Pos) -> u8;
    /// Vanilla `GameRules.RULE_SPREAD_VINES`; standalone test worlds default to true.
    fn spread_vines(&self) -> bool {
        true
    }
    fn request_tree(&mut self, _request: TreeGrowthRequest) {}
}
struct RegionRandomWorld<'a> {
    fluid: RegionFluidWorld<'a>,
    tree_requests: Vec<TreeGrowthRequest>,
}
impl RandomWorld for RegionRandomWorld<'_> {
    fn block(&self, p: Pos) -> u32 {
        FluidWorld::block(&self.fluid, p.0, p.1, p.2)
    }
    fn set_block(&mut self, p: Pos, state: u32) {
        FluidWorld::set_block(&mut self.fluid, p.0, p.1, p.2, state);
    }
    fn brightness(&self, p: Pos) -> u8 {
        RegionFluidWorld::brightness(&self.fluid, p.0, p.1, p.2)
    }
    fn spread_vines(&self) -> bool {
        RegionFluidWorld::spread_vines(&self.fluid)
    }
    fn request_tree(&mut self, request: TreeGrowthRequest) {
        self.tree_requests.push(request);
    }
}
type Handler = fn(&mut dyn RandomWorld, Pos, u32, &mut Rng);
fn no_op(_: &mut dyn RandomWorld, _: Pos, _: u32, _: &mut Rng) {}
fn state_name(state: u32) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    ferrite_data::blocks::state(u16::try_from(state).ok()?)
}
fn named(state: u32, name: &str) -> bool {
    short_name(state) == Some(name)
}
fn short_name(state: u32) -> Option<&'static str> {
    state_name(state).map(|(n, _)| n.strip_prefix("minecraft:").unwrap_or(n))
}
fn property(state: u32, key: &str, value: &str) -> bool {
    state_name(state).is_some_and(|(_, props)| props.iter().any(|&(k, v)| k == key && v == value))
}
fn default_block(name: &str) -> u32 {
    u32::from(ferrite_data::blocks::default_state(name).expect("vanilla block"))
}

/// Adapts the random-tick world to `fire::FireWorld` so `LavaFluid.randomTick` can light fire.
struct FireView<'a>(&'a mut dyn RandomWorld);

impl super::fire::FireWorld for FireView<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        u16::try_from(self.0.block((x, y, z))).ok()
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        self.0.set_block((x, y, z), u32::from(state));
    }
    fn owns(&self, _: i32, _: i32) -> bool {
        true
    }
}

// Vanilla `LavaFluid.randomTick`: lava beside flammable blocks starts a fire and schedules it.
fn lava_tick(
    world: &mut dyn RandomWorld,
    pos: Pos,
    rng: &mut Rng,
    fire: &mut super::fire::FireTicks,
) {
    let mut view = FireView(world);
    if let Some(target) = super::fire::lava_ignites(&mut view, [pos.0, pos.1, pos.2], rng) {
        let delay = super::fire::tick_delay(rng);
        fire.schedule(target, delay);
    }
}
// SpreadingSnowyBlock.canStayAlive / canPropagate, including LightEngine.getLightDampeningInto.
fn can_stay(world: &dyn RandomWorld, p: Pos, grass: u32) -> bool {
    let above = world.block((p.0, p.1 + 1, p.2));
    if named(above, "snow") && property(above, "layers", "1") {
        return true;
    }
    let props = ferrite_data::block_props::get(above as u16);
    if props.fluid.is_some_and(|(_, amount, _)| amount == 8) || props.light_opacity >= 15 {
        return false;
    }
    let grass_props = ferrite_data::block_props::get(grass as u16);
    let face = |state: u32, prop: &ferrite_data::block_props::BlockProps, dir| {
        if prop.can_occlude && prop.use_shape_for_light_occlusion {
            ferrite_data::block_props::face_occlusion(state as u16, dir)
        } else {
            &[]
        }
    };
    let a = face(grass, grass_props, 3);
    let b = face(above, props, 2);
    let mut xs = vec![0.0, 1.0];
    let mut zs = vec![0.0, 1.0];
    for v in a.iter().chain(b) {
        xs.extend([v[0], v[3]]);
        zs.extend([v[2], v[5]]);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    zs.sort_by(f64::total_cmp);
    zs.dedup();
    !xs.windows(2).all(|x| {
        zs.windows(2).all(|z| {
            let (x, z) = ((x[0] + x[1]) * 0.5, (z[0] + z[1]) * 0.5);
            a.iter()
                .chain(b)
                .any(|v| x >= v[0] && x < v[3] && z >= v[2] && z < v[5])
        })
    })
}
// Vanilla 26.3 SpreadingSnowyBlock.randomTick (older name: SpreadingSnowyDirtBlock).
fn grass_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if !can_stay(world, p, state) {
        world.set_block(p, default_block("dirt"));
        return;
    }
    if world.brightness((p.0, p.1 + 1, p.2)) < 9 {
        return;
    }
    for _ in 0..4 {
        let target = (
            p.0 + rng.next_int(3) - 1,
            p.1 + rng.next_int(5) - 3,
            p.2 + rng.next_int(3) - 1,
        );
        let above = world.block((target.0, target.1 + 1, target.2));
        let water = matches!(
            ferrite_data::block_props::get(above as u16).fluid,
            Some((
                ferrite_data::block_props::FluidKind::Water
                    | ferrite_data::block_props::FluidKind::FlowingWater,
                _,
                _
            ))
        );
        if named(world.block(target), "dirt") && !water && can_stay(world, target, state) {
            let snowy = named(above, "snow") || named(above, "snow_block");
            let grass = ferrite_data::blocks::state_id(
                "grass_block",
                &[("snowy", if snowy { "true" } else { "false" })],
            )
            .expect("grass state");
            world.set_block(target, u32::from(grass));
        }
    }
}
// LeavesBlock.isRandomlyTicking / randomTick; item drops are not available yet.
fn leaves_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, _: &mut Rng) {
    if property(state, "distance", "7") && property(state, "persistent", "false") {
        world.set_block(
            p,
            ferrite_fluid::block_for(ferrite_fluid::fluid_state(state)),
        );
    }
}

/// Vanilla `VineBlock` (26.3 jar) random tick. The `spread_vines` rule gates the whole tick;
/// then one try in four picks a random direction. A horizontal face spreads onto a sturdy
/// support or grows down by copying random faces; `up` either attaches to the block above or,
/// when that cell is empty, spreads a random subset of the current faces upward.
fn vine_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if !world.spread_vines() || rng.next_int(4) != 0 {
        return;
    }
    const DIRECTIONS: [((i32, i32, i32), &str); 6] = [
        ((0, -1, 0), "down"),
        ((0, 1, 0), "up"),
        ((0, 0, -1), "north"),
        ((0, 0, 1), "south"),
        ((-1, 0, 0), "west"),
        ((1, 0, 0), "east"),
    ];
    let ((dx, dy, dz), dir) = DIRECTIONS[rng.next_int(6) as usize];
    if dir == "down" {
        if p.1 <= MIN_WORLD_Y {
            return;
        }
        let below = (p.0, p.1 - 1, p.2);
        let below_state = world.block(below);
        if named(below_state, "air") || named(below_state, "vine") {
            let source = if named(below_state, "air") {
                default_block("vine")
            } else {
                below_state
            };
            let copied = vine_copy_random_faces(state, source, rng);
            if copied != source && vine_has_horizontal(copied) {
                world.set_block(below, copied);
            }
        }
        return;
    }
    if dir == "up" {
        if p.1 >= MAX_WORLD_Y {
            return;
        }
        if vine_supports(world, (p.0, p.1 + 1, p.2), "up") {
            if let Some(next) = with_property(state, "up", "true") {
                world.set_block(p, next);
            }
            return;
        }
        let above = (p.0, p.1 + 1, p.2);
        if named(world.block(above), "air") && vine_can_spread(world, p) {
            let mut next = default_block("vine");
            for (face, (fx, fz)) in VINE_HORIZONTAL {
                let neighbour = (above.0 + fx, above.1, above.2 + fz);
                if (rng.next_int(2) == 0 || !vine_supports(world, neighbour, face))
                    && let Some(cleared) = with_property(next, face, "false")
                {
                    next = cleared;
                }
            }
            if vine_has_horizontal(next) {
                world.set_block(above, next);
            }
        }
        return;
    }
    if property(state, dir, "true") || !vine_can_spread(world, p) {
        return;
    }
    let target = (p.0 + dx, p.1 + dy, p.2 + dz);
    if !named(world.block(target), "air") {
        if vine_supports(world, target, dir)
            && let Some(next) = with_property(state, dir, "true")
        {
            world.set_block(p, next);
        }
        return;
    }
    let (clock, counter) = vine_clockwise(dir);
    let (cdx, cdz) = vine_delta(clock);
    let (ccdx, ccdz) = vine_delta(counter);
    let has_clock = property(state, clock, "true");
    let has_counter = property(state, counter, "true");
    if has_clock && vine_supports(world, (target.0 + cdx, target.1, target.2 + cdz), clock) {
        if let Some(next) = with_property(default_block("vine"), clock, "true") {
            world.set_block(target, next);
        }
    } else if has_counter
        && vine_supports(world, (target.0 + ccdx, target.1, target.2 + ccdz), counter)
    {
        if let Some(next) = with_property(default_block("vine"), counter, "true") {
            world.set_block(target, next);
        }
    } else if rng.next_f32() < 0.05
        && vine_supports(world, (target.0, target.1 + 1, target.2), "up")
        && let Some(next) = with_property(default_block("vine"), "up", "true")
    {
        world.set_block(target, next);
    }
}

const VINE_HORIZONTAL: [(&str, (i32, i32)); 4] = [
    ("north", (0, -1)),
    ("east", (1, 0)),
    ("south", (0, 1)),
    ("west", (-1, 0)),
];
const MIN_WORLD_Y: i32 = -64;
const MAX_WORLD_Y: i32 = 319;

fn vine_has_horizontal(state: u32) -> bool {
    VINE_HORIZONTAL
        .iter()
        .any(|(face, _)| property(state, face, "true"))
}

fn vine_copy_random_faces(from: u32, to: u32, rng: &mut Rng) -> u32 {
    let mut out = to;
    for (face, _) in VINE_HORIZONTAL {
        if rng.next_int(2) == 0
            && property(from, face, "true")
            && let Some(next) = with_property(out, face, "true")
        {
            out = next;
        }
    }
    out
}

fn vine_can_spread(world: &dyn RandomWorld, p: Pos) -> bool {
    let mut count = 0;
    for dx in -4..=4 {
        for dy in -1..=1 {
            for dz in -4..=4 {
                if named(world.block((p.0 + dx, p.1 + dy, p.2 + dz)), "vine") {
                    count += 1;
                    if count > 4 {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// The subset of vanilla `MultifaceBlock.canAttachTo` this block model can answer: the support
/// must cover the whole face presented to the vine. A full cube always does; a slab covers its
/// top or bottom face but not its sides; a fence has no full face and stays unsupported.
fn vine_supports(world: &dyn RandomWorld, p: Pos, face: &str) -> bool {
    let state = world.block(p);
    if named(state, "air") || named(state, "vine") {
        return false;
    }
    let Ok(state) = u16::try_from(state) else {
        return false;
    };
    let face = vine_opposite(face);
    ferrite_data::block_props::collision_shape(state)
        .iter()
        .any(|shape| face_is_covered(shape, face))
}

/// The direction opposite `face`; `face` points from the vine toward its support.
fn vine_opposite(face: &str) -> &'static str {
    match face {
        "down" => "up",
        "up" => "down",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        "east" => "west",
        _ => "",
    }
}

/// Whether a collision box covers its whole face at the block boundary. A bottom slab's down
/// face qualifies, while its side and a top slab's down face do not.
fn face_is_covered(shape: &[f64; 6], face: &str) -> bool {
    const EPSILON: f64 = 1e-9;
    let x = shape[0] <= EPSILON && shape[3] >= 1.0 - EPSILON;
    let y = shape[1] <= EPSILON && shape[4] >= 1.0 - EPSILON;
    let z = shape[2] <= EPSILON && shape[5] >= 1.0 - EPSILON;
    match face {
        "down" => shape[1] <= EPSILON && x && z,
        "up" => shape[4] >= 1.0 - EPSILON && x && z,
        "north" => shape[2] <= EPSILON && x && y,
        "south" => shape[5] >= 1.0 - EPSILON && x && y,
        "west" => shape[0] <= EPSILON && z && y,
        "east" => shape[3] >= 1.0 - EPSILON && z && y,
        _ => false,
    }
}

fn vine_clockwise(face: &str) -> (&'static str, &'static str) {
    match face {
        "north" => ("east", "west"),
        "east" => ("south", "north"),
        "south" => ("west", "east"),
        "west" => ("north", "south"),
        _ => ("north", "south"),
    }
}

fn vine_delta(face: &str) -> (i32, i32) {
    match face {
        "north" => (0, -1),
        "east" => (1, 0),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => (0, 0),
    }
}

fn property_value(state: u32, key: &str) -> Option<&'static str> {
    state_name(state)?
        .1
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

fn age(state: u32) -> u32 {
    property_value(state, "age")
        .and_then(|age| age.parse().ok())
        .unwrap_or(0)
}

/// The same block state at another age, keeping its other properties (`BlockState.setValue`).
fn with_age(state: u32, new_age: u32) -> Option<u32> {
    let (name, properties) = state_name(state)?;
    let age = new_age.to_string();
    let properties: Vec<(&str, &str)> = properties
        .iter()
        .map(|&(key, value)| {
            if key == "age" {
                (key, age.as_str())
            } else {
                (key, value)
            }
        })
        .collect();
    ferrite_data::blocks::state_id(name, &properties).map(u32::from)
}

/// `CropBlock.getMaxAge` for the subclasses handled here.
fn crop_max_age(name: &str) -> Option<u32> {
    match name {
        "wheat" | "carrots" | "potatoes" => Some(7),
        "beetroots" => Some(3),
        "torchflower_crop" => Some(1),
        _ => None,
    }
}

/// Vanilla 26.3 `CropBlock.getGrowthSpeed`: farmland under the 3x3 area below, then the
/// neighbouring-crop penalty (`f / 2` when two perpendicular neighbours are the same crop).
fn growth_speed(world: &dyn RandomWorld, p: Pos, crop: &str) -> f32 {
    let mut speed = 1.0f32;
    let below = (p.0, p.1 - 1, p.2);
    for dx in -1..=1 {
        for dz in -1..=1 {
            let mut factor = 0.0f32;
            let state = world.block((below.0 + dx, below.1, below.2 + dz));
            if named(state, "farmland") {
                factor = 1.0;
                if property_value(state, "moisture")
                    .and_then(|moisture| moisture.parse::<u8>().ok())
                    .unwrap_or(0)
                    > 0
                {
                    factor = 3.0;
                }
            }
            if dx != 0 || dz != 0 {
                factor /= 4.0;
            }
            speed += factor;
        }
    }
    let west = (p.0 - 1, p.1, p.2);
    let east = (p.0 + 1, p.1, p.2);
    let north = (p.0, p.1, p.2 - 1);
    let south = (p.0, p.1, p.2 + 1);
    let same = |pos| named(world.block(pos), crop);
    let horizontal = same(west) || same(east);
    let vertical = same(north) || same(south);
    let diagonal = [
        (p.0 - 1, p.2 - 1),
        (p.0 + 1, p.2 - 1),
        (p.0 + 1, p.2 + 1),
        (p.0 - 1, p.2 + 1),
    ]
    .iter()
    .any(|&(x, z)| same((x, p.1, z)));
    if (horizontal && vertical) || diagonal {
        speed /= 2.0;
    }
    speed
}

/// Vanilla `CropBlock.randomTick`, including torchflower's extra one-in-three gate.
fn crop_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    let Some(name) = short_name(state) else {
        return;
    };
    let Some(max_age) = crop_max_age(name) else {
        return;
    };
    let current = age(state);
    if current >= max_age {
        return;
    }
    if name == "torchflower_crop" && rng.next_int(3) != 0 {
        return;
    }
    if world.brightness(p) < 9 {
        return;
    }
    let speed = growth_speed(world, p, name);
    if rng.next_int((25.0 / speed) as i32 + 1) != 0 {
        return;
    }
    if let Some(next) = with_age(state, current + 1) {
        world.set_block(p, next);
    }
}

/// Vanilla `NetherWartBlock.randomTick`: no light requirement, one in ten per random tick.
fn nether_wart_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    let current = age(state);
    if current < 3
        && rng.next_int(10) == 0
        && let Some(next) = with_age(state, current + 1)
    {
        world.set_block(p, next);
    }
}

/// Vanilla `SweetBerryBushBlock.randomTick`.
fn sweet_berry_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    let current = age(state);
    if current < 3
        && rng.next_int(5) == 0
        && world.brightness((p.0, p.1 + 1, p.2)) >= 9
        && let Some(next) = with_age(state, current + 1)
    {
        world.set_block(p, next);
    }
}

/// Vanilla `CocoaBlock.randomTick`: no light requirement.
fn cocoa_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if rng.next_int(5) != 0 {
        return;
    }
    let current = age(state);
    if current < 2
        && let Some(next) = with_age(state, current + 1)
    {
        world.set_block(p, next);
    }
}
/// The same block state with one property changed (`BlockState.setValue`).
fn with_property(state: u32, key: &str, value: &'static str) -> Option<u32> {
    let (name, properties) = state_name(state)?;
    let properties: Vec<(&str, &str)> = properties
        .iter()
        .map(|&(name, old)| {
            if name == key {
                (name, value)
            } else {
                (name, old)
            }
        })
        .collect();
    ferrite_data::blocks::state_id(name, &properties).map(u32::from)
}

/// Vanilla `SugarCaneBlock.randomTick` / `CactusBlock.randomTick`: every random tick advances the
/// age by one, and age 15 places a new block above and resets the age to zero. Both stop at a
/// column of three.
fn stalk_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, _: &mut Rng) {
    let above = (p.0, p.1 + 1, p.2);
    if !named(world.block(above), "air") {
        return;
    }
    let height = column_height(world, p, &[short_name(state).unwrap_or("")], 3);
    if height >= 3 {
        return;
    }
    if age(state) == 15 {
        world.set_block(above, default_block(short_name(state).unwrap_or("")));
        if let Some(reset) = with_age(state, 0) {
            world.set_block(p, reset);
        }
    } else if let Some(next) = with_age(state, age(state) + 1) {
        world.set_block(p, next);
    }
}

/// Vanilla `BambooStalkBlock.randomTick` (26.3 jar): only a stage-0 stalk tries, one in three,
/// into an empty lit cell, and only below the 16-block cap. `growBamboo` places the new tip with
/// the leaves and stage that the height and the two blocks below it select.
fn bamboo_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if property(state, "stage", "1") || rng.next_int(3) != 0 {
        return;
    }
    let above = (p.0, p.1 + 1, p.2);
    if !named(world.block(above), "air") || world.brightness(above) < 9 {
        return;
    }
    let height = column_height(world, p, &["bamboo"], 16);
    if height >= 16 {
        return;
    }
    grow_bamboo(world, p, state, rng, height);
}

/// Vanilla `BambooStalkBlock.growBamboo`: the new tip's `leaves` is `large` when the stalk below
/// already has leaves, `small` when it is the first leafy tip, and `none` at the base. A second
/// leafy tip rewrites the two blocks below to `small`/`none` so the leaves stay on the top pair.
fn grow_bamboo(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng, height: u32) {
    let below_pos = (p.0, p.1 - 1, p.2);
    let below2_pos = (p.0, p.1 - 2, p.2);
    let below = world.block(below_pos);
    let below2 = world.block(below2_pos);
    let mut leaves = "none";
    if height >= 1 {
        if named(below, "bamboo") && !property(below, "leaves", "none") {
            leaves = "large";
            if named(below2, "bamboo") {
                if let Some(small) = with_property(below, "leaves", "small") {
                    world.set_block(below_pos, small);
                }
                if let Some(none) = with_property(below2, "leaves", "none") {
                    world.set_block(below2_pos, none);
                }
            }
        } else {
            leaves = "small";
        }
    }
    let age = if property(state, "age", "1") || named(below2, "bamboo") {
        1
    } else {
        0
    };
    let stage = if (height >= 11 && rng.next_f32() < 0.25) || height == 15 {
        1
    } else {
        0
    };
    if let Some(next) = bamboo_state(age, leaves, stage) {
        world.set_block((p.0, p.1 + 1, p.2), next);
    }
}

fn bamboo_state(age: u32, leaves: &str, stage: u32) -> Option<u32> {
    let age = age.to_string();
    let stage = stage.to_string();
    ferrite_data::blocks::state_id(
        "bamboo",
        &[
            ("age", age.as_str()),
            ("leaves", leaves),
            ("stage", stage.as_str()),
        ],
    )
    .map(u32::from)
}

#[cfg(test)]
fn bamboo_stalk(age: u32) -> Option<u32> {
    bamboo_state(age, "none", 0)
}

/// Number of blocks in the column below `p` (including `p`) that are one of `names`.
fn column_height(world: &dyn RandomWorld, p: Pos, names: &[&str], limit: u32) -> u32 {
    let mut height = 1;
    while height < limit {
        let below = (p.0, p.1 - height as i32, p.2);
        if names.iter().any(|&name| named(world.block(below), name)) {
            height += 1;
        } else {
            break;
        }
    }
    height
}

/// Vanilla `KelpBlock` / `GrowingPlantHeadBlock.randomTick`: one in ten random ticks grows the
/// head into a body block and places a new head in the water above. The column is capped at 25.
fn kelp_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if rng.next_int(10) != 0 {
        return;
    }
    let above = (p.0, p.1 + 1, p.2);
    if !named(world.block(above), "water") {
        return;
    }
    if column_height(world, p, &["kelp", "kelp_plant"], 25) >= 25 {
        return;
    }
    world.set_block(p, default_block("kelp_plant"));
    let next_age = (age(state) + 1).min(25).to_string();
    if let Some(head) = ferrite_data::blocks::state_id("kelp", &[("age", next_age.as_str())]) {
        world.set_block(above, u32::from(head));
    }
}

/// Vanilla `StemBlock.randomTick`. While the stem is young it uses the crop growth speed; at age
/// seven it tries to put its fruit on one horizontal neighbour standing on farmland or a dirt
/// block, and turns into the attached stem facing that neighbour.
fn stem_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    let Some(name) = short_name(state) else {
        return;
    };
    if world.brightness(p) < 9 {
        return;
    }
    let speed = growth_speed(world, p, name);
    if rng.next_int((25.0 / speed) as i32 + 1) != 0 {
        return;
    }
    let current = age(state);
    if current < 7 {
        if let Some(next) = with_age(state, current + 1) {
            world.set_block(p, next);
        }
        return;
    }
    // `Direction.Plane.HORIZONTAL.getRandomDirection`: north, east, south, west.
    let directions = [
        ((0, 0, -1), "north"),
        ((1, 0, 0), "east"),
        ((0, 0, 1), "south"),
        ((-1, 0, 0), "west"),
    ];
    let ((dx, _, dz), facing) = directions[rng.next_int(4) as usize];
    let target = (p.0 + dx, p.1, p.2 + dz);
    if !named(world.block(target), "air") {
        return;
    }
    let below = (target.0, target.1 - 1, target.2);
    if ![
        "farmland",
        "dirt",
        "coarse_dirt",
        "rooted_dirt",
        "moss_block",
        "grass_block",
    ]
    .iter()
    .any(|&floor| named(world.block(below), floor))
    {
        return;
    }
    let (fruit, attached) = if name == "melon_stem" {
        ("melon", "attached_melon_stem")
    } else if name == "pumpkin_stem" {
        ("pumpkin", "attached_pumpkin_stem")
    } else {
        return;
    };
    world.set_block(target, default_block(fruit));
    if let Some(stem) = ferrite_data::blocks::state_id(attached, &[("facing", facing)]) {
        world.set_block(p, u32::from(stem));
    }
}

/// Vanilla `PitcherCropBlock.randomTick`: a lower half younger than four grows with the crop
/// growth speed, and from age three puts the matching upper half above it. Pitcher crop states
/// 0..=2 exist only as a lower half; 3 and 4 are a lower and an upper half.
fn pitcher_crop_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    if property(state, "half", "upper") || age(state) >= 4 {
        return;
    }
    if world.brightness(p) < 9 {
        return;
    }
    let speed = growth_speed(world, p, "pitcher_crop");
    if rng.next_int((25.0 / speed) as i32 + 1) != 0 {
        return;
    }
    let next = age(state) + 1;
    if let Some(lower) = with_age(state, next) {
        world.set_block(p, lower);
    }
    let next_age = next.to_string();
    if next >= 3 {
        let above = (p.0, p.1 + 1, p.2);
        let above_state = world.block(above);
        if (named(above_state, "air") || named(above_state, "pitcher_crop"))
            && let Some(upper) = ferrite_data::blocks::state_id(
                "pitcher_crop",
                &[("age", next_age.as_str()), ("half", "upper")],
            )
        {
            world.set_block(above, u32::from(upper));
        }
    }
}

/// Maps a 26.3 sapling block name to its primary worldgen configured-feature id for the worldgen
/// host (`op = 8`, `place_feature`). Returns `None` when the block is not a known sapling species.
pub fn sapling_feature_id(block_name: &str) -> Option<&'static str> {
    match block_name.trim_start_matches("minecraft:") {
        "oak_sapling" => Some("minecraft:oak"),
        "spruce_sapling" => Some("minecraft:spruce"),
        "birch_sapling" => Some("minecraft:birch"),
        "jungle_sapling" => Some("minecraft:jungle_tree_no_vine"),
        "acacia_sapling" => Some("minecraft:acacia"),
        "dark_oak_sapling" => Some("minecraft:dark_oak"),
        "cherry_sapling" => Some("minecraft:cherry"),
        "pale_oak_sapling" => Some("minecraft:pale_oak_bonemeal"),
        "mangrove_propagule" => Some("minecraft:mangrove"),
        "poplar_sapling" => Some("minecraft:orange_poplar"),
        "azalea" | "flowering_azalea" => Some("minecraft:azalea_tree"),
        _ => None,
    }
}

/// Searches the four 2x2 offsets containing `p` (`TreeGrower.isTwoByTwoSapling`), returning the
/// northwest origin and all four sapling cells `(pos, state)` when all four match `species`.
fn find_two_by_two(world: &dyn RandomWorld, p: Pos, species: &str) -> Option<TwoByTwoFootprint> {
    for (dx, dz) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
        let ox = p.0 + dx;
        let oz = p.2 + dz;
        let coords = [
            [ox, p.1, oz],
            [ox + 1, p.1, oz],
            [ox, p.1, oz + 1],
            [ox + 1, p.1, oz + 1],
        ];
        let mut cells = Vec::with_capacity(4);
        let mut all_match = true;
        for pos in coords {
            let s = world.block((pos[0], pos[1], pos[2]));
            if short_name(s) == Some(species) {
                cells.push((pos, s));
            } else {
                all_match = false;
                break;
            }
        }
        if all_match {
            return Some(([ox, p.1, oz], cells));
        }
    }
    None
}

/// Vanilla `TreeGrower.hasFlowers`: checks `[-2..=2, -1..=1, -2..=2]` around `p` for
/// `#minecraft:flowers`.
fn has_nearby_flowers(world: &dyn RandomWorld, p: Pos) -> bool {
    for dy in -1..=1 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                let s = world.block((p.0 + dx, p.1 + dy, p.2 + dz));
                if short_name(s).is_some_and(|name| {
                    ferrite_data::tags::contains("block", "minecraft:flowers", name)
                }) {
                    return true;
                }
            }
        }
    }
    false
}

/// Picks the 26.3 configured feature and sapling footprint for a stage-1 sapling at `p`.
/// Returns `None` for 2x2-only species (`dark_oak_sapling`, `pale_oak_sapling`) without a 2x2
/// square, or hanging mangrove propagules.
fn select_sapling_tree(
    world: &dyn RandomWorld,
    p: Pos,
    state: u32,
    rng: &mut Rng,
) -> Option<TreeGrowthRequest> {
    let name = short_name(state)?;
    let base_feature = sapling_feature_id(name)?;
    let single = || ([p.0, p.1, p.2], vec![([p.0, p.1, p.2], state)]);
    match name {
        "oak_sapling" => {
            let (origin, saplings) = single();
            let fancy = rng.next_int(10) == 0;
            let flowers = has_nearby_flowers(world, p);
            let feature = match (fancy, flowers) {
                (true, true) => "minecraft:fancy_oak_bees_005",
                (true, false) => "minecraft:fancy_oak",
                (false, true) => "minecraft:oak_bees_005",
                (false, false) => base_feature,
            };
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature,
            })
        }
        "spruce_sapling" => {
            if let Some((origin, saplings)) = find_two_by_two(world, p, "spruce_sapling") {
                let feature = if rng.next_int(2) == 0 {
                    "minecraft:mega_spruce"
                } else {
                    "minecraft:mega_pine"
                };
                Some(TreeGrowthRequest {
                    origin,
                    saplings,
                    feature,
                })
            } else {
                let (origin, saplings) = single();
                Some(TreeGrowthRequest {
                    origin,
                    saplings,
                    feature: base_feature,
                })
            }
        }
        "birch_sapling" => {
            let (origin, saplings) = single();
            let feature = if has_nearby_flowers(world, p) {
                "minecraft:birch_bees_005"
            } else {
                base_feature
            };
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature,
            })
        }
        "jungle_sapling" => {
            if let Some((origin, saplings)) = find_two_by_two(world, p, "jungle_sapling") {
                Some(TreeGrowthRequest {
                    origin,
                    saplings,
                    feature: "minecraft:mega_jungle_tree",
                })
            } else {
                let (origin, saplings) = single();
                Some(TreeGrowthRequest {
                    origin,
                    saplings,
                    feature: base_feature,
                })
            }
        }
        "acacia_sapling" => {
            let (origin, saplings) = single();
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature: base_feature,
            })
        }
        "dark_oak_sapling" | "pale_oak_sapling" => {
            let (origin, saplings) = find_two_by_two(world, p, name)?;
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature: base_feature,
            })
        }
        "cherry_sapling" => {
            let (origin, saplings) = single();
            let feature = if has_nearby_flowers(world, p) {
                "minecraft:cherry_bees_005"
            } else {
                base_feature
            };
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature,
            })
        }
        "mangrove_propagule" => {
            if property(state, "hanging", "true") {
                return None;
            }
            let (origin, saplings) = single();
            let feature = if rng.next_f32() < 0.85 {
                "minecraft:tall_mangrove"
            } else {
                base_feature
            };
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature,
            })
        }
        "poplar_sapling" => {
            let (origin, saplings) = single();
            // `TreeGrower.POPLAR` gives red, orange and yellow one unit of weight each.
            let feature = match rng.next_int(3) {
                0 => "minecraft:red_poplar",
                1 => "minecraft:orange_poplar",
                _ => "minecraft:yellow_poplar",
            };
            Some(TreeGrowthRequest {
                origin,
                saplings,
                feature,
            })
        }
        _ => None,
    }
}

/// Vanilla `SaplingBlock.randomTick` / `MangrovePropaguleBlock.randomTick`: with light at the cell
/// above and one chance in seven, stage 0 advances to stage 1 and stage 1 requests tree growth
/// from the worldgen feature host. Hanging mangrove propagules age 0 -> 4 instead of growing.
fn sapling_tick(world: &mut dyn RandomWorld, p: Pos, state: u32, rng: &mut Rng) {
    let mangrove = named(state, "mangrove_propagule");
    if mangrove && property(state, "hanging", "true") {
        // `MangrovePropaguleBlock.randomTick` ages a hanging propagule on every random tick
        // (no 1/7 roll) and stops once it is fully grown.
        let current = age(state);
        if current < 4
            && let Some(next) = with_age(state, current + 1)
        {
            world.set_block(p, next);
        }
        return;
    }
    // `SaplingBlock.randomTick` requires light at the cell above, but
    // `MangrovePropaguleBlock` overrides it and grows planted propagules without that gate.
    if !mangrove && world.brightness((p.0, p.1 + 1, p.2)) < 9 {
        return;
    }
    if rng.next_int(7) != 0 {
        return;
    }
    if property(state, "stage", "0")
        && let Some(next) = with_property(state, "stage", "1")
    {
        world.set_block(p, next);
    } else if let Some(request) = select_sapling_tree(world, p, state, rng) {
        world.request_tree(request);
    }
}

pub(super) fn sapling_empty_replacement(sapling_state: u32) -> u32 {
    if property(sapling_state, "waterlogged", "true") {
        default_block("water")
    } else {
        default_block("air")
    }
}

/// Captures Ferrite's live blocks around `origin` for the worldgen host, clearing the sapling
/// cell(s) to air/water as `SaplingBlock.advanceTree` does before `ConfiguredFeature.place`.
pub(super) fn capture_tree_context(
    source: &AnvilChunkSource,
    origin: [i32; 3],
    saplings: &[([i32; 3], u32)],
) -> super::generator::TreeContext {
    let min_x = origin[0] - 8;
    let max_x = origin[0] + 8;
    let min_z = origin[2] - 8;
    let max_z = origin[2] + 8;
    let dim_min_y = source.dimension().min_y();
    let dim_max_y = dim_min_y + source.dimension().height() - 1;
    let min_y = (origin[1] - 4).max(dim_min_y);
    let max_y = (origin[1] + 34).min(dim_max_y);
    let sx = (max_x - min_x + 1) as usize;
    let sy = (max_y - min_y + 1).max(1) as usize;
    let sz = (max_z - min_z + 1) as usize;
    let stone = default_block("stone");
    let mut blocks = vec![stone; sx * sy * sz];

    let cmin_x = min_x.div_euclid(16);
    let cmax_x = max_x.div_euclid(16);
    let cmin_z = min_z.div_euclid(16);
    let cmax_z = max_z.div_euclid(16);
    for cx in cmin_x..=cmax_x {
        for cz in cmin_z..=cmax_z {
            let x_start = min_x.max(cx * 16);
            let x_end = max_x.min(cx * 16 + 15);
            let z_start = min_z.max(cz * 16);
            let z_end = max_z.min(cz * 16 + 15);
            source.with_loaded_chunk(cx, cz, |chunk| {
                for y in min_y..=max_y {
                    let dy = (y - min_y) as usize;
                    for z in z_start..=z_end {
                        let dz = (z - min_z) as usize;
                        let lz = z.rem_euclid(16) as u8;
                        for x in x_start..=x_end {
                            let dx = (x - min_x) as usize;
                            let lx = x.rem_euclid(16) as u8;
                            if let Some(state) = chunk.block(lx, y, lz) {
                                blocks[(dy * sz + dz) * sx + dx] = state;
                            }
                        }
                    }
                }
            });
        }
    }

    for &(pos, sapling_state) in saplings {
        if pos[0] >= min_x
            && pos[0] <= max_x
            && pos[1] >= min_y
            && pos[1] <= max_y
            && pos[2] >= min_z
            && pos[2] <= max_z
        {
            let dx = (pos[0] - min_x) as usize;
            let dy = (pos[1] - min_y) as usize;
            let dz = (pos[2] - min_z) as usize;
            blocks[(dy * sz + dz) * sx + dx] = sapling_empty_replacement(sapling_state);
        }
    }

    super::generator::TreeContext {
        min: [min_x, min_y, min_z],
        size: [sx, sy, sz],
        blocks,
    }
}

fn is_structural_tree_block(state: u16) -> bool {
    let Some((name, _)) = ferrite_data::blocks::state(state) else {
        return false;
    };
    ferrite_data::tags::contains("block", "minecraft:logs", name)
}

fn is_replaceable_by_tree(old: u32) -> bool {
    let Ok(old_u16) = u16::try_from(old) else {
        return false;
    };
    if usize::from(old_u16) < ferrite_data::block_props::STATE_COUNT
        && ferrite_data::block_props::get(old_u16).is_air
    {
        return true;
    }
    let Some((name, _)) = ferrite_data::blocks::state(old_u16) else {
        return false;
    };
    ferrite_data::tags::contains("block", "minecraft:replaceable_by_trees", name)
}

fn can_tree_replace_above(old: u32, new_state: u16) -> bool {
    if is_replaceable_by_tree(old) {
        return true;
    }
    let Ok(old_u16) = u16::try_from(old) else {
        return false;
    };
    let (Some((old_name, _)), Some((new_name, _))) = (
        ferrite_data::blocks::state(old_u16),
        ferrite_data::blocks::state(new_state),
    ) else {
        return false;
    };
    (new_name == "minecraft:mangrove_log"
        && ferrite_data::tags::contains(
            "block",
            "minecraft:mangrove_logs_can_grow_through",
            old_name,
        ))
        || (matches!(
            new_name,
            "minecraft:mangrove_roots" | "minecraft:muddy_mangrove_roots"
        ) && ferrite_data::tags::contains(
            "block",
            "minecraft:mangrove_roots_can_grow_through",
            old_name,
        ))
}

fn can_tree_replace_below(old: u32, new_state: u16) -> bool {
    let Ok(old_u16) = u16::try_from(old) else {
        return false;
    };
    let (Some((old_name, _)), Some((new_name, _))) = (
        ferrite_data::blocks::state(old_u16),
        ferrite_data::blocks::state(new_state),
    ) else {
        return false;
    };
    match new_name {
        "minecraft:dirt"
        | "minecraft:coarse_dirt"
        | "minecraft:rooted_dirt"
        | "minecraft:podzol"
        | "minecraft:mycelium" => {
            ferrite_data::tags::contains(
                "block",
                "minecraft:beneath_tree_podzol_replaceable",
                old_name,
            ) || ferrite_data::tags::contains("block", "minecraft:dirt", old_name)
                || matches!(
                    old_name,
                    "minecraft:grass_block"
                        | "minecraft:farmland"
                        | "minecraft:mycelium"
                        | "minecraft:podzol"
                        | "minecraft:mud"
                        | "minecraft:clay"
                )
        }
        "minecraft:muddy_mangrove_roots" => {
            matches!(old_name, "minecraft:mud" | "minecraft:muddy_mangrove_roots")
        }
        "minecraft:mangrove_roots" => {
            is_replaceable_by_tree(old)
                || ferrite_data::tags::contains(
                    "block",
                    "minecraft:mangrove_roots_can_grow_through",
                    old_name,
                )
        }
        _ => can_tree_replace_above(old, new_state),
    }
}

/// Validates a host-placed tree against Ferrite's live world state so stale host terrain or
/// newly built obstructions never overwrite solid blocks or leave partial trunks.
pub(super) fn validate_tree_placement(
    source: &AnvilChunkSource,
    saplings: &[([i32; 3], u32)],
    placed: super::generator::Placed,
) -> Option<ValidatedTree> {
    if saplings.is_empty() || placed.blocks.is_empty() {
        return None;
    }
    let mut sapling_map = HashMap::with_capacity(saplings.len());
    for &(pos, expected) in saplings {
        if source.block(pos[0], pos[1], pos[2]) != Some(expected) {
            return None;
        }
        sapling_map.insert(pos, expected);
    }
    let min_sapling_y = saplings.iter().map(|(p, _)| p[1]).min().unwrap_or(0);

    let mut chosen: HashMap<[i32; 3], (u32, u16)> = HashMap::new();
    let mut order: Vec<[i32; 3]> = Vec::new();
    let mut has_structural = false;

    for (pos, new_state) in placed.blocks {
        let structural = is_structural_tree_block(new_state);
        let Some(old) = source.block(pos[0], pos[1], pos[2]) else {
            if structural || sapling_map.contains_key(&pos) {
                return None;
            }
            continue;
        };
        let allowed = if sapling_map.contains_key(&pos) {
            true
        } else if pos[1] < min_sapling_y {
            can_tree_replace_below(old, new_state)
        } else {
            can_tree_replace_above(old, new_state)
        };
        if !allowed {
            if structural {
                return None;
            }
            continue;
        }
        if structural {
            has_structural = true;
        }
        if !chosen.contains_key(&pos) {
            order.push(pos);
        }
        chosen.insert(pos, (old, new_state));
    }

    if !has_structural {
        return None;
    }

    for &(pos, sapling_state) in saplings {
        if let std::collections::hash_map::Entry::Vacant(entry) = chosen.entry(pos) {
            let empty = u16::try_from(sapling_empty_replacement(sapling_state)).ok()?;
            order.push(pos);
            entry.insert((sapling_state, empty));
        }
    }

    let blocks: Vec<([i32; 3], u32, u16)> = order
        .into_iter()
        .filter_map(|pos| {
            chosen
                .get(&pos)
                .filter(|&&(old, new_state)| old != u32::from(new_state))
                .map(|&(old, new_state)| (pos, old, new_state))
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    let entities = placed
        .entities
        .into_iter()
        .filter(|(pos, _)| chosen.contains_key(pos))
        .collect();
    Some(ValidatedTree { blocks, entities })
}

fn handlers() -> &'static HashMap<u32, Handler> {
    static TABLE: OnceLock<HashMap<u32, Handler>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..ferrite_data::block_props::STATE_COUNT)
            .filter_map(|id| {
                let id = u16::try_from(id).ok()?;
                if !ferrite_data::block_props::get(id).randomly_ticking {
                    return None;
                }
                let state = u32::from(id);
                let handler: Handler = if named(state, "grass_block") {
                    grass_tick
                } else if state_name(state).is_some_and(|(n, _)| n.ends_with("_leaves")) {
                    leaves_tick
                } else if named(state, "sugar_cane") || named(state, "cactus") {
                    stalk_tick
                } else if named(state, "bamboo") {
                    bamboo_tick
                } else if named(state, "kelp") {
                    kelp_tick
                } else if named(state, "melon_stem") || named(state, "pumpkin_stem") {
                    stem_tick
                } else if named(state, "pitcher_crop") {
                    pitcher_crop_tick
                } else if named(state, "vine") {
                    vine_tick
                } else if state_name(state).is_some_and(|(name, _)| {
                    ferrite_data::tags::contains("block", "minecraft:saplings", name)
                }) {
                    sapling_tick
                } else if short_name(state).and_then(crop_max_age).is_some() {
                    crop_tick
                } else if named(state, "nether_wart") {
                    nether_wart_tick
                } else if named(state, "sweet_berry_bush") {
                    sweet_berry_tick
                } else if named(state, "cocoa") {
                    cocoa_tick
                } else {
                    no_op
                };
                Some((state, handler))
            })
            .collect()
    })
}
// Collect base Y coordinates of sections that contain randomly-ticking blocks.
fn ticking_sections(chunk: &Chunk, out: &mut Vec<i32>) {
    out.clear();
    out.extend(
        chunk
            .sections()
            .iter()
            .enumerate()
            .filter_map(|(i, section)| {
                section
                    .is_randomly_ticking()
                    .then_some(chunk.min_y() + i as i32 * 16)
            }),
    );
}
fn sample_positions(
    sections: &[i32],
    cx: i32,
    cz: i32,
    speed: u32,
    mut next: impl FnMut() -> usize,
    mut visit: impl FnMut(Pos),
) {
    for &base_y in sections {
        for _ in 0..speed {
            let index = next();
            visit((
                cx * 16 + (index & 15) as i32,
                base_y + ((index >> 8) & 15) as i32,
                cz * 16 + ((index >> 4) & 15) as i32,
            ));
        }
    }
}
/// Per-tick settings that stay fixed while a region samples its chunks.
pub(super) struct RandomTickOptions {
    pub speed: u32,
    pub spread_vines: bool,
    pub turtle_hatch_chance: f32,
}

/// A turtle-egg random-tick effect the region turns into a sound or particles.
pub(super) enum TurtleEggEvent {
    /// The hatch counter advanced; play the crack sound.
    Crack([i32; 3]),
    /// The cluster hatched; play the hatch sound, break particles and spawn the babies.
    Hatch { at: [i32; 3], state: u16, eggs: u8 },
}

pub(super) struct RandomTicks {
    rng: Rng,
    /// Stage-1 sapling origins waiting on an in-flight worldgen feature host request.
    pub(super) pending_trees: HashSet<[i32; 3]>,
}
impl Default for RandomTicks {
    fn default() -> Self {
        static SEED: AtomicU64 = AtomicU64::new(0x91e10da5c79e7b1d);
        Self {
            rng: Rng::new(SEED.fetch_add(0x9e3779b97f4a7c15, Ordering::Relaxed)),
            pending_trees: HashSet::new(),
        }
    }
}
impl RandomTicks {
    pub fn merge(&mut self, other: Self) {
        self.pending_trees.extend(other.pending_trees);
    }

    pub fn split(&mut self, mut in_sections: impl FnMut([i32; 3]) -> bool) -> Self {
        let (moving, staying): (HashSet<_>, HashSet<_>) = std::mem::take(&mut self.pending_trees)
            .into_iter()
            .partition(|&pos| in_sections(pos));
        self.pending_trees = staying;
        Self {
            rng: Rng::default(),
            pending_trees: moving,
        }
    }

    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        held: &HashMap<ChunkPos, usize>,
        tracking: &Tracking,
        signals: UnboundedSender<Signal>,
        options: RandomTickOptions,
        fire: &mut super::fire::FireTicks,
    ) -> Vec<TurtleEggEvent> {
        let RandomTickOptions {
            speed,
            spread_vines,
            turtle_hatch_chance,
        } = options;
        let mut world = RegionRandomWorld {
            fluid: RegionFluidWorld::new(source, held, tracking)
                .with_spread_vines(spread_vines)
                .with_signals(signals.clone())
                .with_batched_updates(),
            tree_requests: Vec::new(),
        };
        let mut turtle_eggs = Vec::new();
        let mut sections = Vec::with_capacity(24);
        for chunk in held.keys() {
            let Some(mut chunk_arc) = world.fluid.cached_chunk_arc(chunk.0, chunk.1) else {
                continue;
            };
            ticking_sections(&chunk_arc, &mut sections);
            if sections.is_empty() {
                continue;
            }
            let mut seen_gen = world.fluid.write_gen();
            // Sample and dispatch immediately: handler RNG draws precede the next position draw.
            let rng = std::cell::RefCell::new(&mut self.rng);
            sample_positions(
                &sections,
                chunk.0,
                chunk.1,
                speed,
                || rng.borrow_mut().next_int(4096) as usize,
                |pos| {
                    if world.fluid.write_gen() != seen_gen {
                        if let Some(fresh) = world.fluid.cached_chunk_arc(chunk.0, chunk.1) {
                            chunk_arc = fresh;
                        }
                        seen_gen = world.fluid.write_gen();
                    }
                    let Some(state) =
                        chunk_arc.block((pos.0 & 15) as u8, pos.1, (pos.2 & 15) as u8)
                    else {
                        return;
                    };
                    let Ok(id) = u16::try_from(state) else {
                        return;
                    };
                    if usize::from(id) >= ferrite_data::block_props::STATE_COUNT {
                        return;
                    }
                    let props = ferrite_data::block_props::get(id);
                    if props.randomly_ticking && ferrite_entity::turtle::egg_props(id).is_some() {
                        // `TurtleEggBlock.randomTick`: crack twice, then hatch on a night roll.
                        use ferrite_entity::turtle::HatchStep;
                        match ferrite_entity::turtle::hatch_tick(
                            id,
                            &mut rng.borrow_mut(),
                            turtle_hatch_chance,
                        ) {
                            HatchStep::None => {}
                            HatchStep::Cracked(next) => {
                                RandomWorld::set_block(&mut world, pos, u32::from(next));
                                turtle_eggs.push(TurtleEggEvent::Crack([pos.0, pos.1, pos.2]));
                            }
                            HatchStep::Hatched { eggs } => {
                                RandomWorld::set_block(&mut world, pos, 0);
                                turtle_eggs.push(TurtleEggEvent::Hatch {
                                    at: [pos.0, pos.1, pos.2],
                                    state: id,
                                    eggs,
                                });
                            }
                        }
                    } else if props.randomly_ticking
                        && let Some(handler) = handlers().get(&state)
                    {
                        handler(&mut world, pos, state, &mut rng.borrow_mut());
                    }
                    // Vanilla `LavaFluid.randomTick`: lava ignites nearby flammable blocks.
                    if matches!(
                        props.fluid,
                        Some((
                            ferrite_data::block_props::FluidKind::Lava
                                | ferrite_data::block_props::FluidKind::FlowingLava,
                            _,
                            _
                        ))
                    ) {
                        lava_tick(&mut world, pos, &mut rng.borrow_mut(), fire);
                    }
                },
            );
        }
        let requests = std::mem::take(&mut world.tree_requests);
        world.fluid.flush_updates();
        drop(world);
        if !requests.is_empty() && source.terrain().is_some() {
            for request in requests {
                if self.pending_trees.insert(request.origin) {
                    let context = capture_tree_context(source, request.origin, &request.saplings);
                    let _ = signals.send(Signal::GrowSapling {
                        world: source.world_id(),
                        dimension: source.dimension(),
                        origin: request.origin,
                        saplings: request.saplings,
                        feature: request.feature,
                        context,
                    });
                }
            }
        }
        turtle_eggs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_world::{BlockFlags, BlockInfo, Chunk};
    struct Info;
    impl BlockInfo for Info {
        fn flags(&self, id: u32) -> BlockFlags {
            BlockFlags {
                air: id == 0,
                ..Default::default()
            }
        }
        fn randomly_ticking(&self, id: u32) -> bool {
            id == u32::from(ferrite_data::blocks::default_state("grass_block").unwrap())
        }
    }
    #[test]
    fn non_ticking_sections_do_not_sample_and_ticking_sections_use_one_draw_per_pick() {
        let mut chunk = Chunk::new(-64, 32, 0, 0, 16, 6).unwrap();
        let grass = u32::from(ferrite_data::blocks::default_state("grass_block").unwrap());
        chunk.sections_mut()[1].set_block(0, grass, &Info).unwrap();
        let mut draws = 0;
        let mut events = Vec::new();
        let mut sections = Vec::new();
        ticking_sections(&chunk, &mut sections);
        sample_positions(
            &sections,
            2,
            -1,
            3,
            || {
                draws += 1;
                0
            },
            |pos| events.push(pos),
        );
        assert_eq!(draws, 3);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|&pos| pos == (32, -48, -16)));
    }
    struct Grid {
        blocks: HashMap<(i32, i32, i32), u32>,
        light: u8,
        spread_vines: bool,
        requests: Vec<TreeGrowthRequest>,
    }
    impl Default for Grid {
        fn default() -> Self {
            Self {
                blocks: HashMap::new(),
                light: 0,
                spread_vines: true,
                requests: Vec::new(),
            }
        }
    }
    impl RandomWorld for Grid {
        fn block(&self, p: (i32, i32, i32)) -> u32 {
            *self.blocks.get(&p).unwrap_or(&state("air"))
        }
        fn set_block(&mut self, p: (i32, i32, i32), s: u32) {
            self.blocks.insert(p, s);
        }
        fn brightness(&self, _: (i32, i32, i32)) -> u8 {
            self.light
        }
        fn spread_vines(&self) -> bool {
            self.spread_vines
        }
        fn request_tree(&mut self, request: TreeGrowthRequest) {
            self.requests.push(request);
        }
    }
    fn state(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).unwrap())
    }
    #[test]
    fn grass_decays_under_opaque_block_and_spreads_only_in_light() {
        let mut grid = Grid {
            light: 15,
            ..Grid::default()
        };
        let p = (0, 0, 0);
        let target = (1, 0, 0);
        let grass = state("grass_block");
        grid.set_block(p, grass);
        grid.set_block((0, 1, 0), state("stone"));
        grass_tick(&mut grid, p, grass, &mut Rng::new(1));
        assert_eq!(grid.block(p), state("dirt"));
        grid.set_block(p, grass);
        grid.set_block((0, 1, 0), state("air"));
        grid.set_block(target, state("dirt"));
        grid.light = 0;
        let mut rng = Rng::new(12);
        for _ in 0..1000 {
            grass_tick(&mut grid, p, grass, &mut rng);
        }
        assert_eq!(grid.block(target), state("dirt"));
        grid.light = 15;
        for _ in 0..1000 {
            grass_tick(&mut grid, p, grass, &mut rng);
        }
        assert_eq!(grid.block(target), grass);
    }
    fn block_state(name: &str, age: u32) -> u32 {
        let age = age.to_string();
        u32::from(
            ferrite_data::blocks::state_id(name, &[("age", age.as_str())])
                .unwrap_or_else(|| panic!("vanilla {name}")),
        )
    }

    fn farmland(moisture: u8) -> u32 {
        let moisture = moisture.to_string();
        u32::from(
            ferrite_data::blocks::state_id("farmland", &[("moisture", moisture.as_str())])
                .expect("vanilla farmland"),
        )
    }

    #[test]
    fn wheat_grows_in_light_to_max_age_and_stops() {
        assert!(handlers().contains_key(&block_state("wheat", 0)));
        assert!(handlers().contains_key(&block_state("nether_wart", 0)));
        assert!(handlers().contains_key(&block_state("cocoa", 0)));
        let mut grid = Grid {
            light: 15,
            ..Grid::default()
        };
        let p = (0, 0, 0);
        grid.set_block((0, -1, 0), farmland(7));
        grid.set_block(p, block_state("wheat", 0));
        let mut rng = Rng::new(7);
        for _ in 0..5000 {
            let state = grid.block(p);
            crop_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(age(grid.block(p)), 7);
        let max = grid.block(p);
        for _ in 0..1000 {
            crop_tick(&mut grid, p, max, &mut rng);
        }
        assert_eq!(grid.block(p), max);
    }

    #[test]
    fn wheat_does_not_grow_without_light() {
        let mut grid = Grid::default();
        let p = (0, 0, 0);
        grid.set_block((0, -1, 0), farmland(7));
        grid.set_block(p, block_state("wheat", 0));
        let mut rng = Rng::new(11);
        for _ in 0..1000 {
            let state = grid.block(p);
            crop_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(age(grid.block(p)), 0);
    }

    #[test]
    fn nether_wart_grows_without_light() {
        let mut grid = Grid::default();
        let p = (0, 0, 0);
        grid.set_block(p, block_state("nether_wart", 0));
        let mut rng = Rng::new(3);
        for _ in 0..5000 {
            let state = grid.block(p);
            nether_wart_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(age(grid.block(p)), 3);
    }

    #[test]
    fn cocoa_keeps_its_facing_when_it_ages() {
        let mut grid = Grid::default();
        let p = (0, 0, 0);
        let cocoa = u32::from(
            ferrite_data::blocks::state_id("cocoa", &[("age", "1"), ("facing", "east")])
                .expect("vanilla cocoa"),
        );
        grid.set_block(p, cocoa);
        let mut rng = Rng::new(5);
        for _ in 0..1000 {
            let state = grid.block(p);
            cocoa_tick(&mut grid, p, state, &mut rng);
        }
        let grown = grid.block(p);
        assert_eq!(age(grown), 2);
        assert_eq!(property_value(grown, "facing"), Some("east"));
    }

    #[test]
    fn leaves_decay_only_when_nonpersistent_and_distance_seven() {
        let mut grid = Grid::default();
        let p = (0, 0, 0);
        let leaf = |persistent, distance| {
            u32::from(
                ferrite_data::blocks::state_id(
                    "oak_leaves",
                    &[("persistent", persistent), ("distance", distance)],
                )
                .unwrap(),
            )
        };
        let mut rng = Rng::new(1);
        for state in [leaf("true", "7"), leaf("false", "6")] {
            grid.set_block(p, state);
            leaves_tick(&mut grid, p, state, &mut rng);
            assert_eq!(grid.block(p), state);
        }
        let state = leaf("false", "7");
        grid.set_block(p, state);
        leaves_tick(&mut grid, p, state, &mut rng);
        assert_eq!(grid.block(p), super::tests::state("air"));
    }

    fn growth_grid() -> Grid {
        Grid {
            light: 15,
            ..Grid::default()
        }
    }

    #[test]
    fn sugar_cane_and_cactus_grow_one_block_per_sixteen_ticks_and_stop_at_three() {
        for name in ["sugar_cane", "cactus"] {
            let mut grid = growth_grid();
            let p = (0, 0, 0);
            grid.set_block(p, block_state(name, 0));
            let mut rng = Rng::new(1);
            // Random ticks reach every stalk; each age-15 top places one block above and
            // resets, so sixteen ticks per block.
            let mut top = 0;
            for _ in 0..80 {
                while named(grid.block((0, top + 1, 0)), name) {
                    top += 1;
                }
                let state = grid.block((0, top, 0));
                stalk_tick(&mut grid, (0, top, 0), state, &mut rng);
            }
            assert_eq!(column_height(&grid, (0, top, 0), &[name], 4), 3, "{name}");
            let landed = top;
            for _ in 0..80 {
                while named(grid.block((0, top + 1, 0)), name) {
                    top += 1;
                }
                let state = grid.block((0, top, 0));
                stalk_tick(&mut grid, (0, top, 0), state, &mut rng);
            }
            assert_eq!(top, landed, "{name} capped");
            assert_eq!(column_height(&grid, (0, top, 0), &[name], 4), 3, "{name}");
        }
    }

    #[test]
    fn bamboo_grows_to_sixteen_and_ignores_a_stage_one_top() {
        let mut grid = growth_grid();
        let p = (0, 0, 0);
        grid.set_block(p, bamboo_stalk(0).unwrap());
        let mut rng = Rng::new(1);
        let mut top = 0;
        // Vanilla tries once in three, so 400 calls are not enough for 15 growth steps.
        for _ in 0..4000 {
            while named(grid.block((0, top + 1, 0)), "bamboo") {
                top += 1;
            }
            let state = grid.block((0, top, 0));
            bamboo_tick(&mut grid, (0, top, 0), state, &mut rng);
        }
        let height = column_height(&grid, (0, top, 0), &["bamboo"], 20);
        // `growBamboo` gives a 25% chance of stage 1 at heights 11..=14, so a particular stalk
        // may stop before 16; the hard cap is still 16.
        assert!((11..=16).contains(&height), "bamboo height: {height}");
        assert!(
            property(grid.block((0, top, 0)), "stage", "1"),
            "the stalk stopped only because its tip reached stage 1"
        );
        // A stage-1 stalk is a finished shoot and is not grown by random ticks.
        let full = ferrite_data::blocks::state_id(
            "bamboo",
            &[("age", "0"), ("leaves", "large"), ("stage", "1")],
        )
        .unwrap();
        let high = 20;
        grid.set_block((0, high, 0), u32::from(full));
        bamboo_tick(&mut grid, (0, high, 0), u32::from(full), &mut rng);
        assert_eq!(grid.block((0, high + 1, 0)), super::tests::state("air"));
        // The hard cap: a stage-0 tip at height 16 cannot place a seventeenth block.
        let mut capped = growth_grid();
        for y in 0..16 {
            capped.set_block((0, y, 0), bamboo_stalk(0).unwrap());
        }
        let tip = capped.block((0, 15, 0));
        bamboo_tick(&mut capped, (0, 15, 0), tip, &mut rng);
        assert_eq!(capped.block((0, 16, 0)), super::tests::state("air"));
    }

    #[test]
    fn bamboo_leaves_move_to_the_new_tip() {
        let mut grid = growth_grid();
        let mut rng = Rng::new(4);
        let base = bamboo_state(0, "none", 0).unwrap();
        let small = bamboo_state(0, "small", 0).unwrap();
        let tip = bamboo_state(0, "none", 0).unwrap();
        grid.set_block((0, 0, 0), base);
        grid.set_block((0, 1, 0), small);
        grid.set_block((0, 2, 0), tip);
        grow_bamboo(&mut grid, (0, 2, 0), tip, &mut rng, 3);
        assert!(property(grid.block((0, 3, 0)), "leaves", "large"));
        assert!(property(grid.block((0, 1, 0)), "leaves", "small"));
        assert!(property(grid.block((0, 0, 0)), "leaves", "none"));
    }

    #[test]
    fn kelp_grows_a_body_block_and_a_new_head_in_water() {
        let mut grid = growth_grid();
        let water = state("water");
        let mut rng = Rng::new(4);
        grid.set_block((0, 0, 0), block_state("kelp", 0));
        let mut y = 0;
        for _ in 0..20_000 {
            let head = grid.block((0, y, 0));
            if !named(head, "kelp") {
                break;
            }
            grid.set_block((0, y + 1, 0), water);
            kelp_tick(&mut grid, (0, y, 0), head, &mut rng);
            if named(grid.block((0, y, 0)), "kelp_plant") {
                y += 1;
            }
        }
        assert!(y >= 3, "kelp grew only {y} blocks");
        assert!(y < 25, "kelp exceeded the vanilla height cap: {y}");
        assert!(named(grid.block((0, y, 0)), "kelp"));
        assert!(named(grid.block((0, y - 1, 0)), "kelp_plant"));
    }

    #[test]
    fn a_mature_stem_places_its_fruit_on_a_dirt_neighbour() {
        let mut grid = growth_grid();
        let p = (0, 0, 0);
        grid.set_block(p, block_state("melon_stem", 0));
        for (x, z) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            grid.set_block((x, -1, z), state("farmland"));
        }
        let mut rng = Rng::new(9);
        for _ in 0..20_000 {
            let stem = grid.block(p);
            if !named(stem, "melon_stem") {
                break;
            }
            stem_tick(&mut grid, p, stem, &mut rng);
        }
        let fruit = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .find(|&(x, z)| named(grid.block((x, 0, z)), "melon"));
        assert!(fruit.is_some(), "the stem never placed a melon");
        let attached = grid.block(p);
        assert!(named(attached, "attached_melon_stem"), "{attached}");
        assert!(property_value(attached, "facing").is_some());
    }

    #[test]
    fn a_pitcher_crop_grows_a_matching_upper_half() {
        let mut grid = growth_grid();
        let p = (0, 0, 0);
        let lower =
            ferrite_data::blocks::state_id("pitcher_crop", &[("age", "0"), ("half", "lower")])
                .unwrap();
        grid.set_block(p, u32::from(lower));
        grid.set_block((0, -1, 0), state("farmland"));
        let mut rng = Rng::new(2);
        for _ in 0..20_000 {
            let state = grid.block(p);
            if age(state) >= 4 {
                break;
            }
            pitcher_crop_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(age(grid.block(p)), 4);
        let upper = grid.block((0, 1, 0));
        assert!(named(upper, "pitcher_crop"), "{upper}");
        assert!(property(upper, "half", "upper"));
        assert_eq!(age(upper), 4, "both halves carry the same age");
    }

    #[test]
    fn a_vine_spreads_down_a_wall_and_keeps_its_face() {
        let mut grid = growth_grid();
        let p = (0, 0, 0);
        let vine = u32::from(
            ferrite_data::blocks::state_id("vine", &[("north", "true")]).expect("vanilla vine"),
        );
        grid.set_block(p, vine);
        grid.set_block((0, 0, -1), state("stone"));
        let mut rng = Rng::new(2);
        for _ in 0..5000 {
            let current = grid.block(p);
            vine_tick(&mut grid, p, current, &mut rng);
            if named(grid.block((0, -1, 0)), "vine") {
                break;
            }
        }
        let below = grid.block((0, -1, 0));
        assert!(named(below, "vine"), "the vine did not grow down");
        assert!(property(below, "north", "true"), "the copied face was lost");
        assert!(vine_supports(&grid, (0, 0, -1), "north"));
        assert!(!vine_supports(&grid, (0, 1, 0), "up"));
    }

    #[test]
    fn a_vine_uses_a_slabs_full_face_but_not_its_side() {
        let mut grid = growth_grid();
        let slab = |kind: &str| {
            u32::from(
                ferrite_data::blocks::state_id("stone_slab", &[("type", kind)])
                    .expect("vanilla stone slab"),
            )
        };
        grid.set_block((0, 1, 0), slab("bottom"));
        assert!(
            vine_supports(&grid, (0, 1, 0), "up"),
            "a bottom slab's underside is a full face"
        );
        grid.set_block((0, 0, -1), slab("bottom"));
        assert!(
            !vine_supports(&grid, (0, 0, -1), "north"),
            "a slab's side is not full"
        );
        grid.set_block((0, 2, 0), slab("top"));
        assert!(
            !vine_supports(&grid, (0, 2, 0), "up"),
            "a top slab's underside is half a block above the boundary"
        );
    }

    #[test]
    fn the_spread_vines_rule_stops_every_vine_growth_face() {
        let mut grid = growth_grid();
        grid.spread_vines = false;
        let p = (0, 0, 0);
        let vine = u32::from(
            ferrite_data::blocks::state_id("vine", &[("north", "true")]).expect("vanilla vine"),
        );
        grid.set_block(p, vine);
        grid.set_block((0, 0, -1), state("stone"));
        let mut rng = Rng::new(2);
        for _ in 0..5000 {
            vine_tick(&mut grid, p, vine, &mut rng);
        }
        assert_eq!(grid.block(p), vine);
        assert_eq!(grid.block((0, -1, 0)), state("air"));
    }

    #[test]
    fn a_sapling_advances_one_stage_in_light() {
        let mut grid = growth_grid();
        let p = (0, 0, 0);
        let sapling = state("oak_sapling");
        grid.set_block(p, sapling);
        let mut rng = Rng::new(3);
        for _ in 0..1000 {
            let state = grid.block(p);
            if property(state, "stage", "1") {
                break;
            }
            sapling_tick(&mut grid, p, state, &mut rng);
        }
        assert!(property(grid.block(p), "stage", "1"));
        for state in [
            block_state("sugar_cane", 0),
            block_state("cactus", 0),
            bamboo_stalk(0).unwrap(),
            block_state("kelp", 0),
            block_state("melon_stem", 0),
            block_state("pumpkin_stem", 0),
            u32::from(
                ferrite_data::blocks::state_id("pitcher_crop", &[("age", "0"), ("half", "lower")])
                    .unwrap(),
            ),
            sapling,
        ] {
            assert!(handlers().contains_key(&state), "no handler for {state}");
        }
        assert!(
            handlers().contains_key(&state("vine")),
            "vine random ticks are registered"
        );
        assert_eq!(
            sapling_feature_id("minecraft:oak_sapling"),
            Some("minecraft:oak")
        );
        assert_eq!(
            sapling_feature_id("cherry_sapling"),
            Some("minecraft:cherry")
        );
        assert_eq!(sapling_feature_id("minecraft:stone"), None);
    }

    #[test]
    fn sapling_feature_ids_and_species_selection_cover_all_supported_species() {
        for (block, expected) in [
            ("minecraft:oak_sapling", "minecraft:oak"),
            ("minecraft:spruce_sapling", "minecraft:spruce"),
            ("minecraft:birch_sapling", "minecraft:birch"),
            ("minecraft:jungle_sapling", "minecraft:jungle_tree_no_vine"),
            ("minecraft:acacia_sapling", "minecraft:acacia"),
            ("minecraft:dark_oak_sapling", "minecraft:dark_oak"),
            ("minecraft:cherry_sapling", "minecraft:cherry"),
            ("minecraft:pale_oak_sapling", "minecraft:pale_oak_bonemeal"),
            ("minecraft:mangrove_propagule", "minecraft:mangrove"),
            ("minecraft:poplar_sapling", "minecraft:orange_poplar"),
            ("minecraft:azalea", "minecraft:azalea_tree"),
            ("minecraft:flowering_azalea", "minecraft:azalea_tree"),
        ] {
            assert_eq!(sapling_feature_id(block), Some(expected), "{block}");
        }
        assert_eq!(sapling_feature_id("minecraft:stone"), None);

        // 1x1 oak with/without nearby flowers, birch, acacia, cherry, poplar, and planted mangrove.
        let stage1 = |name: &str| {
            u32::from(
                ferrite_data::blocks::state_id(name, &[("stage", "1")]).expect("stage-1 sapling"),
            )
        };
        let mut grid = growth_grid();
        let p = (0, 64, 0);
        let mut rng = Rng::new(7);
        for (name, allowed) in [
            ("oak_sapling", &["minecraft:oak", "minecraft:fancy_oak"][..]),
            ("spruce_sapling", &["minecraft:spruce"][..]),
            ("birch_sapling", &["minecraft:birch"][..]),
            ("jungle_sapling", &["minecraft:jungle_tree_no_vine"][..]),
            ("acacia_sapling", &["minecraft:acacia"][..]),
            ("cherry_sapling", &["minecraft:cherry"][..]),
            (
                "poplar_sapling",
                &[
                    "minecraft:orange_poplar",
                    "minecraft:red_poplar",
                    "minecraft:yellow_poplar",
                ][..],
            ),
        ] {
            let s = stage1(name);
            grid.set_block(p, s);
            let req = select_sapling_tree(&grid, p, s, &mut rng).expect(name);
            assert_eq!(req.origin, [0, 64, 0]);
            assert_eq!(req.saplings, vec![([0, 64, 0], s)]);
            assert!(allowed.contains(&req.feature), "{name}: {}", req.feature);
            assert!(handlers().contains_key(&s), "handler missing for {name}");
        }

        // Flower nearby activates bee variants for oak, birch, and cherry.
        grid.set_block((2, 64, 1), state("poppy"));
        let birch = stage1("birch_sapling");
        assert_eq!(
            select_sapling_tree(&grid, p, birch, &mut rng)
                .unwrap()
                .feature,
            "minecraft:birch_bees_005"
        );
        let cherry = stage1("cherry_sapling");
        assert_eq!(
            select_sapling_tree(&grid, p, cherry, &mut rng)
                .unwrap()
                .feature,
            "minecraft:cherry_bees_005"
        );

        // 2x2-only species fail alone and select the NW origin when arranged 2x2.
        for (name, expected_feature) in [
            ("dark_oak_sapling", "minecraft:dark_oak"),
            ("pale_oak_sapling", "minecraft:pale_oak_bonemeal"),
            ("jungle_sapling", "minecraft:mega_jungle_tree"),
        ] {
            let mut g = growth_grid();
            let s = stage1(name);
            g.set_block((10, 64, 10), s);
            if name != "jungle_sapling" {
                assert!(
                    select_sapling_tree(&g, (10, 64, 10), s, &mut rng).is_none(),
                    "{name} must require 2x2"
                );
            }
            for (x, z) in [(9, 9), (10, 9), (9, 10), (10, 10)] {
                g.set_block((x, 64, z), s);
            }
            // Ticking any corner resolves to the NW origin (9, 64, 9) and all four cells.
            let req = select_sapling_tree(&g, (10, 64, 10), s, &mut rng).expect(name);
            assert_eq!(req.origin, [9, 64, 9]);
            assert_eq!(req.saplings.len(), 4);
            assert_eq!(req.feature, expected_feature);
        }

        // 2x2 spruce picks mega_spruce or mega_pine.
        let mut spruce_grid = growth_grid();
        let spruce = stage1("spruce_sapling");
        for (x, z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            spruce_grid.set_block((x, 64, z), spruce);
        }
        let spruce_req = select_sapling_tree(&spruce_grid, (1, 64, 0), spruce, &mut rng).unwrap();
        assert_eq!(spruce_req.origin, [0, 64, 0]);
        assert!(matches!(
            spruce_req.feature,
            "minecraft:mega_spruce" | "minecraft:mega_pine"
        ));

        // Mangrove propagule: planted grows, hanging ages 0..4 without requesting a tree.
        let planted_mangrove = u32::from(
            ferrite_data::blocks::state_id(
                "mangrove_propagule",
                &[
                    ("age", "0"),
                    ("hanging", "false"),
                    ("stage", "1"),
                    ("waterlogged", "true"),
                ],
            )
            .unwrap(),
        );
        let req = select_sapling_tree(&grid, p, planted_mangrove, &mut rng).unwrap();
        assert!(matches!(
            req.feature,
            "minecraft:mangrove" | "minecraft:tall_mangrove"
        ));
        let hanging_mangrove = u32::from(
            ferrite_data::blocks::state_id(
                "mangrove_propagule",
                &[
                    ("age", "0"),
                    ("hanging", "true"),
                    ("stage", "0"),
                    ("waterlogged", "false"),
                ],
            )
            .unwrap(),
        );
        assert!(select_sapling_tree(&grid, p, hanging_mangrove, &mut rng).is_none());
        let mut hang_grid = growth_grid();
        hang_grid.light = 0;
        hang_grid.set_block(p, hanging_mangrove);
        for _ in 0..1000 {
            let cur = hang_grid.block(p);
            sapling_tick(&mut hang_grid, p, cur, &mut rng);
        }
        assert_eq!(age(hang_grid.block(p)), 4);
        assert!(hang_grid.requests.is_empty());
    }

    #[test]
    fn hanging_mangrove_propagule_ages_on_every_random_tick() {
        let hanging = |age: u32| {
            let age = age.to_string();
            u32::from(
                ferrite_data::blocks::state_id(
                    "mangrove_propagule",
                    &[
                        ("age", age.as_str()),
                        ("hanging", "true"),
                        ("stage", "0"),
                        ("waterlogged", "false"),
                    ],
                )
                .expect("vanilla mangrove_propagule"),
            )
        };
        let mut grid = growth_grid();
        grid.light = 0;
        let p = (0, 64, 0);
        grid.set_block(p, hanging(0));
        let mut rng = Rng::new(1);
        for expected in 1..=4 {
            let state = grid.block(p);
            sapling_tick(&mut grid, p, state, &mut rng);
            assert_eq!(
                age(grid.block(p)),
                expected,
                "hanging propagules age on every random tick"
            );
        }
        for _ in 0..64 {
            let state = grid.block(p);
            sapling_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(age(grid.block(p)), 4, "a fully grown propagule stops at 4");
        assert!(grid.requests.is_empty());
    }

    #[test]
    fn planted_mangrove_propagule_ignores_light() {
        let planted = u32::from(
            ferrite_data::blocks::state_id(
                "mangrove_propagule",
                &[
                    ("age", "0"),
                    ("hanging", "false"),
                    ("stage", "1"),
                    ("waterlogged", "false"),
                ],
            )
            .expect("vanilla mangrove_propagule"),
        );
        let mut grid = growth_grid();
        grid.light = 0;
        let p = (0, 64, 0);
        grid.set_block(p, planted);
        let mut rng = Rng::new(5);
        for _ in 0..500 {
            if !grid.requests.is_empty() {
                break;
            }
            let state = grid.block(p);
            sapling_tick(&mut grid, p, state, &mut rng);
        }
        assert_eq!(
            grid.requests.len(),
            1,
            "MangrovePropaguleBlock.randomTick has no brightness gate"
        );
        assert!(matches!(
            grid.requests[0].feature,
            "minecraft:mangrove" | "minecraft:tall_mangrove"
        ));
    }

    #[test]
    fn flowers_one_block_below_the_sapling_activate_bee_trees() {
        let mut grid = growth_grid();
        let p = (0, 64, 0);
        let birch = u32::from(
            ferrite_data::blocks::state_id("birch_sapling", &[("stage", "1")])
                .expect("vanilla birch_sapling"),
        );
        grid.set_block(p, birch);
        grid.set_block((2, 63, 1), state("poppy"));
        assert_eq!(
            select_sapling_tree(&grid, p, birch, &mut Rng::new(7))
                .expect("birch request")
                .feature,
            "minecraft:birch_bees_005",
            "TreeGrower.hasFlowers scans one block below the sapling"
        );
    }

    #[test]
    fn poplar_sapling_weights_red_orange_and_yellow_equally() {
        let poplar = u32::from(
            ferrite_data::blocks::state_id("poplar_sapling", &[("stage", "1")])
                .expect("vanilla poplar_sapling"),
        );
        let mut grid = growth_grid();
        let p = (0, 64, 0);
        grid.set_block(p, poplar);
        let mut rng = Rng::new(11);
        let mut counts = HashMap::new();
        for _ in 0..3000 {
            let feature = select_sapling_tree(&grid, p, poplar, &mut rng)
                .expect("poplar request")
                .feature;
            *counts.entry(feature).or_insert(0usize) += 1;
        }
        for feature in [
            "minecraft:red_poplar",
            "minecraft:orange_poplar",
            "minecraft:yellow_poplar",
        ] {
            let count = counts.get(feature).copied().unwrap_or(0);
            assert!(
                (900..=1100).contains(&count),
                "{feature} should have weight 1 of 3, got {count} of 3000"
            );
        }
    }

    #[test]
    fn stage_one_sapling_requests_tree_only_in_light() {
        let p = (0, 64, 0);
        let stage1 =
            u32::from(ferrite_data::blocks::state_id("oak_sapling", &[("stage", "1")]).unwrap());
        let mut dark = Grid {
            light: 8,
            ..Grid::default()
        };
        dark.set_block(p, stage1);
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            sapling_tick(&mut dark, p, stage1, &mut rng);
        }
        assert!(
            dark.requests.is_empty(),
            "darkness must prevent tree growth"
        );

        let mut lit = growth_grid();
        lit.set_block(p, stage1);
        for _ in 0..200 {
            if !lit.requests.is_empty() {
                break;
            }
            sapling_tick(&mut lit, p, stage1, &mut rng);
        }
        assert_eq!(lit.requests.len(), 1);
        assert_eq!(lit.requests[0].origin, [0, 64, 0]);
        assert_eq!(lit.block(p), stage1, "sapling waits for host placement");
    }
}
