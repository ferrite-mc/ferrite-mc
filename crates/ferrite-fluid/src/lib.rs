//! Vanilla fluid-state conversion and base tick delays.
//!
//! Fluid properties are extracted from the 26.3 server by ferrite-data.
//! Vanilla `LiquidBlock` maps block levels 8..=15 to the same falling fluid
//! state; converting that state back therefore produces canonical level 8.

use ferrite_data::{block_props, blocks};

pub trait FluidWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> u32;
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32);
    fn ultrawarm(&self) -> bool;
    /// Whether the lava source conversion game rule is enabled.
    fn lava_conversion(&self) -> bool {
        false
    }
    /// Collision boxes in block-local coordinates; override for dynamic shapes.
    fn collision_shape(&self, x: i32, y: i32, z: i32) -> Vec<[f64; 6]> {
        u16::try_from(self.block(x, y, z))
            .ok()
            .filter(|&id| usize::from(id) < block_props::STATE_COUNT)
            .map(|id| block_props::collision_shape(id).to_vec())
            .unwrap_or_else(|| vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    Water,
    Lava,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidState {
    pub kind: FluidKind,
    /// Zero for empty; otherwise 1 through 8.
    pub amount: u8,
    pub falling: bool,
    pub source: bool,
}

/// Decode a vanilla block state. Unknown IDs do not contain fluid.
pub fn fluid_state(state_id: u32) -> FluidState {
    let empty = FluidState {
        kind: FluidKind::Empty,
        amount: 0,
        falling: false,
        source: false,
    };
    let Ok(id) = u16::try_from(state_id) else {
        return empty;
    };
    if usize::from(id) >= block_props::STATE_COUNT {
        return empty;
    }
    let Some((kind, amount, falling)) = block_props::get(id).fluid else {
        return empty;
    };
    let (kind, source) = match kind {
        block_props::FluidKind::Water => (FluidKind::Water, true),
        block_props::FluidKind::FlowingWater => (FluidKind::Water, false),
        block_props::FluidKind::Lava => (FluidKind::Lava, true),
        block_props::FluidKind::FlowingLava => (FluidKind::Lava, false),
        block_props::FluidKind::Other(_) => return empty,
    };
    FluidState {
        kind,
        amount,
        falling,
        source,
    }
}

/// Convert to a standalone fluid block, or air for empty. A waterlogged
/// container cannot be recovered from its fluid alone.
///
/// Panics if the state is inconsistent or cannot be represented as a block.
pub fn block_for(fluid: FluidState) -> u32 {
    let name = match fluid.kind {
        FluidKind::Empty => {
            assert!(fluid.amount == 0 && !fluid.source && !fluid.falling);
            return u32::from(blocks::default_state("air").expect("vanilla air"));
        }
        FluidKind::Water => "water",
        FluidKind::Lava => "lava",
    };
    assert!((1..=8).contains(&fluid.amount));
    assert!(!fluid.source || (fluid.amount == 8 && !fluid.falling));
    assert!(fluid.source || fluid.falling || fluid.amount < 8);
    let level = if fluid.source {
        0
    } else {
        8 - fluid.amount + if fluid.falling { 8 } else { 0 }
    };
    u32::from(
        blocks::state_id(name, &[("level", &level.to_string())]).expect("vanilla fluid level"),
    )
}

/// Base fluid tick delay in game ticks; empty has no scheduled fluid tick.
pub const fn tick_delay(kind: FluidKind, ultrawarm: bool) -> u32 {
    match kind {
        FluidKind::Water => 5,
        FluidKind::Lava if ultrawarm => 10,
        FluidKind::Lava => 30,
        FluidKind::Empty => 0,
    }
}

fn props(state: u32) -> Option<&'static block_props::BlockProps> {
    let id = u16::try_from(state).ok()?;
    (usize::from(id) < block_props::STATE_COUNT).then(|| block_props::get(id))
}

/// Approximation of `FlowingFluid.java#canHoldFluid`: replaceable blocks and
/// waterloggable blocks can accept water. Other block-specific container rules
/// are not yet modeled, and existing fluids are accepted only of the same kind.
pub fn can_hold_fluid(state: u32, kind: FluidKind) -> bool {
    if kind == FluidKind::Empty {
        return false;
    }
    let existing = fluid_state(state);
    if existing.kind != FluidKind::Empty {
        return existing.kind == kind;
    }
    let Some(p) = props(state) else {
        return false;
    };
    if p.replaceable {
        return true;
    }
    kind == FluidKind::Water
        && u16::try_from(state)
            .ok()
            .and_then(blocks::state)
            .is_some_and(|(_, properties)| {
                properties
                    .iter()
                    .any(|&(name, value)| name == "waterlogged" && value == "false")
            })
}

/// Direction from the first block to its neighbor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    West,
    East,
    Down,
    Up,
    North,
    South,
}
impl Direction {
    fn face(self) -> (usize, bool) {
        match self {
            Self::West => (0, false),
            Self::East => (0, true),
            Self::Down => (1, false),
            Self::Up => (1, true),
            Self::North => (2, false),
            Self::South => (2, true),
        }
    }
}

/// Checks the union of the two collision faces as in
/// `FlowingFluid.java#canPassThroughWall` and `Shapes.mergedFaceOccludes`.
/// Static shapes are extracted using the vanilla empty block getter; blocks
/// with context-dependent collision shapes need world-aware shape data.
pub fn can_pass_face(from: u32, to: u32, direction: Direction) -> bool {
    let (Ok(from), Ok(to)) = (u16::try_from(from), u16::try_from(to)) else {
        return false;
    };
    if usize::from(from) >= block_props::STATE_COUNT || usize::from(to) >= block_props::STATE_COUNT
    {
        return false;
    }
    faces_pass(
        block_props::collision_shape(from),
        block_props::collision_shape(to),
        direction,
    )
}
/// World-aware version, allowing collision shapes to depend on position and neighbours.
pub fn can_pass_at(
    world: &impl FluidWorld,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    direction: Direction,
) -> bool {
    faces_pass(
        &world.collision_shape(from.0, from.1, from.2),
        &world.collision_shape(to.0, to.1, to.2),
        direction,
    )
}
fn faces_pass(from: &[[f64; 6]], to: &[[f64; 6]], direction: Direction) -> bool {
    let (axis, positive) = direction.face();
    let mut rectangles = Vec::new();
    for (shape, high) in [(from, positive), (to, !positive)] {
        for b in shape {
            let face = if high {
                b[axis + 3] >= 1.0 && b[axis] < 1.0
            } else {
                b[axis] <= 0.0 && b[axis + 3] > 0.0
            };
            if face {
                let (a, c) = match axis {
                    0 => (1, 2),
                    1 => (0, 2),
                    _ => (0, 1),
                };
                let rect = [
                    b[a].max(0.0),
                    b[c].max(0.0),
                    b[a + 3].min(1.0),
                    b[c + 3].min(1.0),
                ];
                if rect[0] < rect[2] && rect[1] < rect[3] {
                    rectangles.push(rect);
                }
            }
        }
    }
    let mut xs = vec![0.0, 1.0];
    let mut ys = vec![0.0, 1.0];
    for r in &rectangles {
        xs.extend([r[0], r[2]]);
        ys.extend([r[1], r[3]]);
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            let (x, y) = ((x[0] + x[1]) / 2.0, (y[0] + y[1]) / 2.0);
            if !rectangles
                .iter()
                .any(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
            {
                return true;
            }
        }
    }
    false
}
/// Compatibility helper for the east face. Prefer [`can_pass_face`].
pub fn can_pass_through(from: u32, to: u32) -> bool {
    can_pass_face(from, to, Direction::East)
}

/// Compute `FlowingFluid.java#getNewLiquid` without changing the world. Source
/// support uses the extracted vanilla `BlockState.isSolid`; face occlusion uses [`can_pass_face`].
/// `lava_conversion` is the lava source-conversion game rule.
pub fn get_new_liquid(
    world: &impl FluidWorld,
    x: i32,
    y: i32,
    z: i32,
    kind: FluidKind,
    lava_conversion: bool,
) -> FluidState {
    let empty = FluidState {
        kind: FluidKind::Empty,
        amount: 0,
        falling: false,
        source: false,
    };
    if kind == FluidKind::Empty {
        return empty;
    }
    let mut max_amount = 0;
    let mut sources = 0;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (Some(nx), Some(nz)) = (x.checked_add(dx), z.checked_add(dz)) else {
            continue;
        };
        let adjacent = world.block(nx, y, nz);
        let fluid = fluid_state(adjacent);
        let direction = match (dx, dz) {
            (1, 0) => Direction::East,
            (-1, 0) => Direction::West,
            (0, 1) => Direction::South,
            _ => Direction::North,
        };
        if fluid.kind == kind && can_pass_at(world, (x, y, z), (nx, y, nz), direction) {
            max_amount = max_amount.max(fluid.amount);
            sources += u8::from(fluid.source);
        }
    }
    if sources >= 2
        && (kind == FluidKind::Water || lava_conversion)
        && let Some(below_y) = y.checked_sub(1)
    {
        let below = world.block(x, below_y, z);
        let below_fluid = fluid_state(below);
        if props(below).is_some_and(|p| p.blocks_motion)
            || (below_fluid.kind == kind && below_fluid.source)
        {
            return FluidState {
                kind,
                amount: 8,
                falling: false,
                source: true,
            };
        }
    }
    if let Some(above_y) = y.checked_add(1) {
        let above = world.block(x, above_y, z);
        if fluid_state(above).kind == kind
            && can_pass_at(world, (x, y, z), (x, above_y, z), Direction::Up)
        {
            return FluidState {
                kind,
                amount: 8,
                falling: true,
                source: false,
            };
        }
    }
    let drop_off = if kind == FluidKind::Lava && !world.ultrawarm() {
        2
    } else {
        1
    };
    let amount = max_amount.saturating_sub(drop_off);
    if amount == 0 {
        empty
    } else {
        FluidState {
            kind,
            amount,
            falling: false,
            source: false,
        }
    }
}

mod tick;
pub use tick::{FluidTicks, Position};

/// Emulates `SimpleWaterloggedBlock.placeLiquid` and a water bucket for
/// standalone replaceable blocks. Returns whether a block was changed.
pub fn place_water(world: &mut impl FluidWorld, pos: Position) -> bool {
    let old = world.block(pos.0, pos.1, pos.2);
    if fluid_state(old).kind != FluidKind::Empty {
        return false;
    }
    if u16::try_from(old)
        .ok()
        .and_then(blocks::state)
        .is_some_and(|(name, _)| name == "minecraft:cauldron" || name == "cauldron")
    {
        if let Some(full) = blocks::state_id("water_cauldron", &[("level", "3")]) {
            world.set_block(pos.0, pos.1, pos.2, u32::from(full));
            return true;
        }
    }
    if let Some(new) = waterlogged_variant(old, true) {
        world.set_block(pos.0, pos.1, pos.2, new);
        return true;
    }
    if props(old).is_some_and(|p| p.replaceable) {
        world.set_block(
            pos.0,
            pos.1,
            pos.2,
            u32::from(blocks::default_state("water").unwrap()),
        );
        return true;
    }
    false
}
/// Emulates source water `BucketPickup` for liquid blocks and waterlogged blocks.
pub fn pickup_water(world: &mut impl FluidWorld, pos: Position) -> bool {
    let old = world.block(pos.0, pos.1, pos.2);
    if let Some((name, properties)) = u16::try_from(old).ok().and_then(blocks::state)
        && (name == "minecraft:water_cauldron" || name == "water_cauldron")
    {
        if properties
            .iter()
            .any(|&(key, value)| key == "level" && value == "3")
        {
            world.set_block(
                pos.0,
                pos.1,
                pos.2,
                u32::from(blocks::default_state("cauldron").unwrap()),
            );
            return true;
        }
        return false;
    }
    let fluid = fluid_state(old);
    if fluid.kind != FluidKind::Water || !fluid.source {
        return false;
    }
    let new = waterlogged_variant(old, false).or_else(|| {
        u16::try_from(old)
            .ok()
            .and_then(blocks::state)
            .and_then(|(name, _)| {
                (name == "minecraft:water" || name == "water")
                    .then(|| u32::from(blocks::default_state("air").unwrap()))
            })
    });
    if let Some(new) = new {
        world.set_block(pos.0, pos.1, pos.2, new);
        true
    } else {
        false
    }
}
fn waterlogged_variant(state: u32, wet: bool) -> Option<u32> {
    let (name, properties) = u16::try_from(state).ok().and_then(blocks::state)?;
    let desired = if wet { "true" } else { "false" };
    if !properties
        .iter()
        .any(|&(k, v)| k == "waterlogged" && v != desired)
    {
        return None;
    }
    let properties: Vec<_> = properties
        .iter()
        .map(|&(k, v)| (k, if k == "waterlogged" { desired } else { v }))
        .collect();
    blocks::state_id(name, &properties).map(u32::from)
}
