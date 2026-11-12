//! Piston movement planning.

use ferrite_data::{block_props, blocks, piston_props};
use ferrite_math::Direction;
use ferrite_nbt::{Compound, Tag};
use std::collections::HashSet;

/// Paper `unsupported-settings.allow-headless-pistons`, set once at startup. While false (the
/// default) removing a piston head also removes the extended base it belonged to, so no headless
/// piston is left standing; true keeps the base, which is what Paper's "allow" does.
pub static ALLOW_HEADLESS_PISTONS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn allow_headless_pistons() -> bool {
    ALLOW_HEADLESS_PISTONS.load(std::sync::atomic::Ordering::Relaxed)
}

fn direction_of(props: &[(&str, &str)]) -> Option<Direction> {
    let facing = props.iter().find(|(key, _)| *key == "facing")?.1;
    Direction::ALL
        .into_iter()
        .find(|direction| facing_name(*direction) == facing)
}

/// The extended piston base a removed head leaves behind, if any (vanilla
/// `PistonHeadBlock` removes the base it was attached to). `old` is the state that stood at
/// `head_pos`, `new` the state that replaced it.
pub fn orphaned_base(
    block: impl Fn([i32; 3]) -> Option<u16>,
    head_pos: [i32; 3],
    old: u16,
    new: u16,
) -> Option<[i32; 3]> {
    let (old_name, old_props) = blocks::state(old)?;
    if old_name.strip_prefix("minecraft:").unwrap_or(old_name) != "piston_head" {
        return None;
    }
    let facing = direction_of(old_props)?;
    if let Some((new_name, new_props)) = blocks::state(new)
        && new_name.strip_prefix("minecraft:").unwrap_or(new_name) == "piston_head"
        && direction_of(new_props) == Some(facing)
    {
        return None;
    }
    let base = offset(head_pos, facing.opposite());
    let (name, props) = blocks::state(block(base)?)?;
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    (matches!(name, "piston" | "sticky_piston")
        && props.contains(&("extended", "true"))
        && direction_of(props) == Some(facing))
    .then_some(base)
}

/// Applies [`orphaned_base`] to a chunk source: clears the base (no drop, like vanilla's
/// `removeBlock`) unless headless pistons are allowed. Returns the cleared position.
pub fn remove_orphaned_base(
    source: &super::anvil::AnvilChunkSource,
    head_pos: [i32; 3],
    old: u16,
    new: u16,
    allow_headless: bool,
) -> Option<[i32; 3]> {
    if allow_headless {
        return None;
    }
    let read = |pos: [i32; 3]| {
        source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
    };
    let base = orphaned_base(read, head_pos, old, new)?;
    let base_state = read(base)?;
    source
        .compare_set_block(base[0], base[1], base[2], Some(u32::from(base_state)), 0)
        .ok()?;
    Some(base)
}

/// Read-only view of blocks owned by the current region. None means unavailable.
pub trait PistonWorld {
    fn block(&self, pos: [i32; 3]) -> Option<u16>;
    fn has_block_entity(&self, pos: [i32; 3]) -> bool;
}

impl PistonWorld for super::fluid::RegionFluidWorld<'_> {
    fn block(&self, pos: [i32; 3]) -> Option<u16> {
        super::redstone::RedstoneWorld::block(self, pos)
    }
    fn has_block_entity(&self, pos: [i32; 3]) -> bool {
        self.source.block_entity(pos[0], pos[1], pos[2]).is_some()
    }
}

/// A complete plan; no world mutation has happened when resolution returns.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub to_move: Vec<[i32; 3]>,
    pub to_destroy: Vec<[i32; 3]>,
}

fn offset([x, y, z]: [i32; 3], direction: Direction) -> [i32; 3] {
    let (dx, dy, dz) = direction.step();
    [x + dx, y + dy, z + dz]
}

fn facing_name(facing: Direction) -> &'static str {
    match facing {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

fn facing_id(facing: Direction) -> i8 {
    match facing {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

/// The block state a moving block entity is attached to (`MovingPistonBlock`).
pub fn moving_state(facing: Direction, sticky: bool) -> Option<u16> {
    blocks::state_id(
        "moving_piston",
        &[
            ("facing", facing_name(facing)),
            ("type", if sticky { "sticky" } else { "normal" }),
        ],
    )
}

/// Vanilla's `BlockState.CODEC` representation used by `PistonMovingBlockEntity.blockState`.
/// A default state is the block name; a state with property changes uses `id`/`properties`.
pub fn block_state_tag(state: u16) -> Option<Tag> {
    let (name, properties) = blocks::state(state)?;
    if blocks::default_state(name) == Some(state) {
        return Some(Tag::String(name.into()));
    }
    let mut tag = Compound::new();
    tag.insert("id".into(), Tag::String(name.into()));
    let mut values = Compound::new();
    for &(key, value) in properties {
        values.insert(key.into(), Tag::String(value.into()));
    }
    tag.insert("properties".into(), Tag::Compound(values));
    Some(Tag::Compound(tag))
}

/// The custom data of a `PistonMovingBlockEntity`, as sent by
/// `ClientboundBlockEntityDataPacket` for `minecraft:piston`.
pub fn moving_entity_nbt(
    moved_state: u16,
    facing: Direction,
    extending: bool,
    progress: f32,
    source_piston: bool,
) -> Option<Compound> {
    let mut data = Compound::new();
    data.insert("blockState".into(), block_state_tag(moved_state)?);
    data.insert("facing".into(), Tag::Byte(facing_id(facing)));
    data.insert("progress".into(), Tag::Float(progress.clamp(0.0, 1.0)));
    data.insert("extending".into(), Tag::Byte(i8::from(extending)));
    data.insert("source".into(), Tag::Byte(i8::from(source_piston)));
    Some(data)
}

pub(super) fn within_border(border: &super::commands::WorldBorderState, pos: [i32; 3]) -> bool {
    const LIMIT: f64 = 29_999_984.0;
    let half = border.diameter / 2.0;
    let x = f64::from(pos[0]);
    let z = f64::from(pos[2]);
    x >= (border.center_x - half).clamp(-LIMIT, LIMIT)
        && x < (border.center_x + half).clamp(-LIMIT, LIMIT)
        && z >= (border.center_z - half).clamp(-LIMIT, LIMIT)
        && z < (border.center_z + half).clamp(-LIMIT, LIMIT)
}

/// Whether a block position lies inside the dimension's build height `[min_y, min_y + height)`.
pub(super) fn within_height(dimension: super::Dimension, pos: [i32; 3]) -> bool {
    pos[1] >= dimension.min_y() && pos[1] < dimension.min_y() + dimension.height()
}

/// Whether `bbox` is clear of every collision shape in the region-owned blocks it covers.
/// Vanilla piston pushes displace an entity into blocks with no collision (air, water, plants)
/// but leave it in place when a solid shape overlaps its destination.
pub fn entity_space_clear(world: &impl PistonWorld, bbox: ferrite_math::AABB) -> bool {
    // Touching a block face is not an overlap, so exclude the boundary cell at the box maximum.
    const EPSILON: f64 = 1.0e-7;
    let min_x = bbox.min.x.floor() as i32;
    let min_y = bbox.min.y.floor() as i32;
    let min_z = bbox.min.z.floor() as i32;
    let max_x = (bbox.max.x - EPSILON).floor() as i32;
    let max_y = (bbox.max.y - EPSILON).floor() as i32;
    let max_z = (bbox.max.z - EPSILON).floor() as i32;
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let Some(state) = world.block([x, y, z]) else {
                    return false;
                };
                for shape in block_props::collision_shape(state) {
                    let block = ferrite_math::AABB::new(
                        f64::from(x) + shape[0],
                        f64::from(y) + shape[1],
                        f64::from(z) + shape[2],
                        f64::from(x) + shape[3],
                        f64::from(y) + shape[4],
                        f64::from(z) + shape[5],
                    );
                    if block.intersects(bbox) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Check direct and quasi-connected power using the redstone engine's directional signal.
pub fn powered(
    pos: [i32; 3],
    facing: Direction,
    mut signal: impl FnMut([i32; 3], Direction) -> bool,
) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| side != facing && signal(pos, side))
        || Direction::ALL
            .into_iter()
            .any(|side| side != Direction::Down && signal(offset(pos, Direction::Up), side))
}

#[derive(PartialEq, Eq)]
enum Reaction {
    Air,
    Move,
    Block,
    Destroy,
    PushOnly,
}

fn reaction(world: &impl PistonWorld, pos: [i32; 3], state: u16) -> Reaction {
    let Some((name, props)) = blocks::state(state) else {
        return Reaction::Block;
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let physical = block_props::get(state);
    if physical.is_air {
        return Reaction::Air;
    }
    let piston = piston_props::get(state);
    if world.has_block_entity(pos) || piston.has_block_entity || physical.hardness < 0.0 {
        return Reaction::Block;
    }
    if matches!(name, "piston" | "sticky_piston") {
        return if props.iter().any(|&(k, v)| k == "extended" && v == "true") {
            Reaction::Block
        } else {
            Reaction::Move
        };
    }
    match piston.reaction {
        piston_props::PushReaction::PushPull | piston_props::PushReaction::IgnoreEntity => {
            Reaction::Move
        }
        piston_props::PushReaction::PushOnly => Reaction::PushOnly,
        piston_props::PushReaction::Destroy => Reaction::Destroy,
        piston_props::PushReaction::Immovable => Reaction::Block,
    }
}

/// The sticky material of a block (`SlimeBlock`/`HoneyBlock`).
fn sticky_kind(state: u16) -> Option<&'static str> {
    let (name, _) = blocks::state(state)?;
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "slime_block" => Some("slime"),
        "honey_block" => Some("honey"),
        _ => None,
    }
}

fn is_sticky(state: u16) -> bool {
    sticky_kind(state).is_some()
}

/// `PistonStructureResolver.canStickToEachOther`: slime and honey never stick to each other,
/// and a pair sticks when at least one side is sticky.
fn can_stick(first: u16, second: u16) -> bool {
    match (sticky_kind(first), sticky_kind(second)) {
        (Some("slime"), Some("honey")) | (Some("honey"), Some("slime")) => false,
        _ => is_sticky(first) || is_sticky(second),
    }
}

fn displaced([x, y, z]: [i32; 3], direction: Direction, distance: i32) -> [i32; 3] {
    let (dx, dy, dz) = direction.step();
    [x + dx * distance, y + dy * distance, z + dz * distance]
}

fn head_state(facing: Direction, sticky: bool) -> Option<u16> {
    blocks::state_id(
        "piston_head",
        &[
            ("facing", facing_name(facing)),
            ("type", if sticky { "sticky" } else { "normal" }),
            ("short", "false"),
        ],
    )
}

/// Resolve an extension or a sticky retraction without mutating the world.
///
/// This follows `PistonStructureResolver`: a sticky block pulls the contiguous line behind it,
/// branches only to its perpendicular sides, and a `PUSH` block is only pushable along the
/// piston's facing. The head cell reads as air during retraction because vanilla removes the
/// head before it builds the pull plan.
pub fn resolve(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
) -> Option<Plan> {
    let mut resolver = Resolver {
        world,
        base,
        extending,
        piston_facing: facing,
        push_direction: if extending { facing } else { facing.opposite() },
        head_cell: offset(base, facing),
        air: blocks::default_state("air")?,
        plan: Plan::default(),
    };
    resolver.run().then_some(resolver.plan)
}

struct Resolver<'a, W: PistonWorld> {
    world: &'a W,
    base: [i32; 3],
    extending: bool,
    piston_facing: Direction,
    push_direction: Direction,
    head_cell: [i32; 3],
    air: u16,
    plan: Plan,
}

impl<W: PistonWorld> Resolver<'_, W> {
    fn run(&mut self) -> bool {
        let start = if self.extending {
            offset(self.base, self.piston_facing)
        } else {
            offset(self.head_cell, self.piston_facing)
        };
        let Some(state) = self.block(start) else {
            return false;
        };
        if !self.pushable(start, state, false, self.piston_facing) {
            if self.extending && reaction(self.world, start, state) == Reaction::Destroy {
                self.plan.to_destroy.push(start);
                return true;
            }
            return false;
        }
        if !self.add_block_line(start, self.push_direction) {
            return false;
        }
        let mut index = 0;
        while index < self.plan.to_move.len() {
            let pos = self.plan.to_move[index];
            let Some(state) = self.block(pos) else {
                return false;
            };
            if is_sticky(state) && !self.add_branching_blocks(pos) {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Region-owned state, with the retracting head cell already treated as removed.
    fn block(&self, pos: [i32; 3]) -> Option<u16> {
        if !self.extending && pos == self.head_cell {
            return Some(self.air);
        }
        self.world.block(pos)
    }

    fn pushable(
        &self,
        pos: [i32; 3],
        state: u16,
        allow_destroy: bool,
        connection_direction: Direction,
    ) -> bool {
        match reaction(self.world, pos, state) {
            Reaction::Air | Reaction::Move => true,
            Reaction::PushOnly => self.push_direction == connection_direction,
            Reaction::Destroy => allow_destroy,
            Reaction::Block => false,
        }
    }

    /// One push line, including the contiguous sticky blocks behind its first block.
    fn add_block_line(&mut self, start: [i32; 3], connection: Direction) -> bool {
        let Some(state) = self.block(start) else {
            return false;
        };
        if reaction(self.world, start, state) == Reaction::Air {
            return true;
        }
        if !self.pushable(start, state, false, connection)
            || start == self.base
            || self.plan.to_move.contains(&start)
        {
            return true;
        }

        let mut line_added = 1usize;
        if line_added + self.plan.to_move.len() > 12 {
            return false;
        }

        let mut next_state = state;
        while is_sticky(next_state) {
            let pos = displaced(start, self.push_direction.opposite(), line_added as i32);
            let previous = next_state;
            let Some(state) = self.block(pos) else {
                return false;
            };
            next_state = state;
            if reaction(self.world, pos, state) == Reaction::Air
                || !can_stick(previous, state)
                || !self.pushable(pos, state, false, self.push_direction.opposite())
                || pos == self.base
            {
                break;
            }
            line_added += 1;
            if line_added + self.plan.to_move.len() > 12 {
                return false;
            }
        }

        for offset in (0..line_added).rev() {
            self.plan.to_move.push(displaced(
                start,
                self.push_direction.opposite(),
                offset as i32,
            ));
        }

        let mut distance = 1;
        loop {
            let pos = displaced(start, self.push_direction, distance);
            if let Some(collision) = self.plan.to_move.iter().position(|queued| *queued == pos) {
                self.reorder_list_at_collision(line_added, collision);
                let end = collision + line_added;
                for index in 0..=end {
                    let branch = self.plan.to_move[index];
                    let Some(state) = self.block(branch) else {
                        return false;
                    };
                    if is_sticky(state) && !self.add_branching_blocks(branch) {
                        return false;
                    }
                }
                return true;
            }
            let Some(state) = self.block(pos) else {
                return false;
            };
            if reaction(self.world, pos, state) == Reaction::Air {
                return true;
            }
            if !self.pushable(pos, state, true, self.push_direction) || pos == self.base {
                return false;
            }
            if reaction(self.world, pos, state) == Reaction::Destroy {
                if !self.plan.to_destroy.contains(&pos) {
                    self.plan.to_destroy.push(pos);
                }
                return true;
            }
            if self.plan.to_move.len() >= 12 {
                return false;
            }
            self.plan.to_move.push(pos);
            line_added += 1;
            distance += 1;
        }
    }

    /// Sticky blocks branch only to their perpendicular sides; the line along the push axis is
    /// already handled by [`Self::add_block_line`].
    fn add_branching_blocks(&mut self, from: [i32; 3]) -> bool {
        let Some(from_state) = self.block(from) else {
            return false;
        };
        for direction in Direction::ALL {
            if direction == self.push_direction || direction == self.push_direction.opposite() {
                continue;
            }
            let neighbour = offset(from, direction);
            let Some(state) = self.block(neighbour) else {
                return false;
            };
            if can_stick(state, from_state) && !self.add_block_line(neighbour, direction) {
                return false;
            }
        }
        true
    }

    /// `PistonStructureResolver.reorderListAtCollision`: keep the newly discovered line in front
    /// of the line it ran into so the later blocks move first.
    fn reorder_list_at_collision(&mut self, line_added: usize, collision: usize) {
        let last_line_start = self.plan.to_move.len() - line_added;
        let mut reordered = Vec::with_capacity(self.plan.to_move.len());
        reordered.extend_from_slice(&self.plan.to_move[..collision]);
        reordered.extend_from_slice(&self.plan.to_move[last_line_start..]);
        reordered.extend_from_slice(&self.plan.to_move[collision..last_line_start]);
        self.plan.to_move = reordered;
    }
}

/// Convert a movement plan into expected-state changes for an atomic block batch.
pub fn changes(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
    plan: &Plan,
) -> Option<Vec<([i32; 3], u32, u32)>> {
    let base_state = world.block(base)?;
    let (base_name, props) = blocks::state(base_state)?;
    let name = base_name.strip_prefix("minecraft:").unwrap_or(base_name);
    if !matches!(name, "piston" | "sticky_piston") {
        return None;
    }
    let facing_name = facing_name(facing);
    if !props.contains(&("facing", facing_name))
        || !props.contains(&("extended", if extending { "false" } else { "true" }))
    {
        return None;
    }
    let next_base = blocks::state_id(
        base_name,
        &[
            ("facing", facing_name),
            ("extended", if extending { "true" } else { "false" }),
        ],
    )?;
    let air = blocks::default_state("air")?;
    let head = offset(base, facing);
    if !extending {
        let (head_name, head_props) = blocks::state(world.block(head)?)?;
        if head_name.strip_prefix("minecraft:").unwrap_or(head_name) != "piston_head"
            || !head_props.contains(&("facing", facing_name))
        {
            return None;
        }
    }
    let mut desired = std::collections::BTreeMap::new();
    desired.insert(base, next_base);
    desired.insert(
        head,
        if extending {
            head_state(facing, name == "sticky_piston")?
        } else {
            air
        },
    );
    for &pos in plan.to_destroy.iter().chain(plan.to_move.iter()) {
        desired.insert(pos, air);
    }
    let direction = if extending { facing } else { facing.opposite() };
    for &pos in &plan.to_move {
        desired.insert(offset(pos, direction), world.block(pos)?);
    }
    // The head takes precedence over a vacated source during extension.
    if extending {
        desired.insert(head, head_state(facing, name == "sticky_piston")?);
    }
    desired
        .into_iter()
        .map(|(pos, next)| Some((pos, u32::from(world.block(pos)?), u32::from(next))))
        .collect()
}

/// One moved block, from the cell it occupied to the cell it will occupy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovedBlock {
    pub source: [i32; 3],
    pub destination: [i32; 3],
    pub state: u16,
}

/// A client-facing moving-piston animation, with the same state a vanilla
/// `PistonMovingBlockEntity` keeps while the move settles.
#[derive(Debug, Clone, PartialEq)]
pub struct MovingBlock {
    pub source: [i32; 3],
    pub destination: [i32; 3],
    pub moved_state: u16,
    pub facing: Direction,
    pub extending: bool,
    pub progress: f32,
    /// True for the moving piston head, whose two-cell movement starts at the base.
    pub source_piston: bool,
}
impl MovingBlock {
    pub fn new(
        source: [i32; 3],
        destination: [i32; 3],
        moved_state: u16,
        facing: Direction,
        extending: bool,
        source_piston: bool,
    ) -> Self {
        Self {
            source,
            destination,
            moved_state,
            facing,
            extending,
            progress: 0.0,
            source_piston,
        }
    }

    /// One server tick of a two-tick movement.
    pub fn tick(&mut self) {
        self.progress = (self.progress + 0.5).min(1.0);
    }

    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }

    pub fn overlaps(&self, affected: &HashSet<[i32; 3]>) -> bool {
        affected.contains(&self.source) || affected.contains(&self.destination)
    }

    /// The `moving_piston` state the client puts at the destination.
    pub fn moving_state(&self) -> Option<u16> {
        let (name, properties) = blocks::state(self.moved_state)?;
        let sticky = name.strip_prefix("minecraft:").unwrap_or(name) == "piston_head"
            && properties
                .iter()
                .any(|&(key, value)| key == "type" && value == "sticky");
        moving_state(self.facing, sticky)
    }

    pub fn entity_nbt(&self) -> Option<Compound> {
        moving_entity_nbt(
            self.moved_state,
            self.facing,
            self.extending,
            self.progress,
            self.source_piston,
        )
    }
}

/// Moving blocks that still need their client animation. The final world state is committed
/// atomically before this overlay is sent; the overlay never changes collision or drops.
#[derive(Debug, Default)]
pub struct Animations {
    blocks: Vec<MovingBlock>,
}
impl Animations {
    /// Replaces any animation that shares a source or destination with the new movement.
    pub fn start(&mut self, blocks: Vec<MovingBlock>, affected: &HashSet<[i32; 3]>) {
        self.blocks.retain(|block| !block.overlaps(affected));
        self.blocks.extend(blocks);
    }

    pub fn advance(&mut self) {
        for block in &mut self.blocks {
            block.tick();
        }
        self.blocks.retain(|block| !block.is_complete());
    }

    pub fn active(&self) -> &[MovingBlock] {
        &self.blocks
    }

    pub fn merge(&mut self, other: Self) {
        for block in other.blocks {
            self.blocks
                .retain(|old| old.destination != block.destination);
            self.blocks.push(block);
        }
    }

    /// Splits out animations whose destination is in the moving section set.
    pub fn split(&mut self, keep: impl Fn([i32; 3]) -> bool) -> Self {
        let mut moved = Vec::new();
        self.blocks.retain(|block| {
            if keep(block.destination) {
                moved.push(block.clone());
                false
            } else {
                true
            }
        });
        Self { blocks: moved }
    }
}

/// Expected block changes and blocks whose drops must be spawned after commit.
pub struct Operation {
    pub edits: Vec<([i32; 3], u32, u32)>,
    pub destroyed: Vec<([i32; 3], u16)>,
    /// Blocks that move together with the head (not including the head itself).
    pub moves: Vec<MovedBlock>,
    /// The extension head, which travels from the base to the head cell.
    pub head: Option<MovedBlock>,
    /// Cells occupied by the piston head or a moved block before entities are pushed.
    pub entity_push_cells: Vec<[i32; 3]>,
}

/// Prepare a complete operation without modifying the world.
pub fn operation(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
) -> Option<Operation> {
    let (name, _) = blocks::state(world.block(base)?)?;
    let sticky = name.strip_prefix("minecraft:").unwrap_or(name) == "sticky_piston";
    let plan = if extending {
        resolve(world, base, facing, true)?
    } else if sticky {
        resolve(world, base, facing, false).unwrap_or_default()
    } else {
        Plan::default()
    };
    let destroyed = plan
        .to_destroy
        .iter()
        .map(|&pos| Some((pos, world.block(pos)?)))
        .collect::<Option<Vec<_>>>()?;
    let direction = if extending { facing } else { facing.opposite() };
    let moves = plan
        .to_move
        .iter()
        .map(|&source| {
            Some(MovedBlock {
                source,
                destination: offset(source, direction),
                state: world.block(source)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let head = if extending {
        Some(MovedBlock {
            source: base,
            destination: offset(base, facing),
            state: head_state(facing, sticky)?,
        })
    } else {
        None
    };
    let mut entity_push_cells = vec![offset(base, facing)];
    entity_push_cells.extend(
        plan.to_move
            .iter()
            .map(|&pos| if extending { offset(pos, facing) } else { pos }),
    );
    Some(Operation {
        edits: changes(world, base, facing, extending, &plan)?,
        destroyed,
        moves,
        head,
        entity_push_cells,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn blocks_cannot_be_pushed_out_of_the_build_height() {
        use super::within_height;
        use crate::game::Dimension;
        let (min, top) = (
            Dimension::Overworld.min_y(),
            Dimension::Overworld.min_y() + Dimension::Overworld.height(),
        );
        assert!(within_height(Dimension::Overworld, [0, min, 0]));
        assert!(within_height(Dimension::Overworld, [0, top - 1, 0]));
        assert!(!within_height(Dimension::Overworld, [0, min - 1, 0]));
        assert!(!within_height(Dimension::Overworld, [0, top, 0]));
        assert!(
            !within_height(Dimension::End, [0, -1, 0]),
            "the end starts at 0"
        );
    }

    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Grid(HashMap<[i32; 3], u16>);
    impl PistonWorld for Grid {
        fn block(&self, pos: [i32; 3]) -> Option<u16> {
            Some(*self.0.get(&pos).unwrap_or(&0))
        }
        fn has_block_entity(&self, _: [i32; 3]) -> bool {
            false
        }
    }
    fn state(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).unwrap()
    }

    #[test]
    fn twelve_blocks_move_but_thirteen_do_not() {
        let mut grid = Grid::default();
        for x in 1..=12 {
            grid.0.insert([x, 64, 0], state("stone"));
        }
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move
                .len(),
            12
        );
        grid.0.insert([13, 64, 0], state("stone"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn immovable_blocks_stop_and_breakable_blocks_are_destroyed() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("obsidian"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
        grid.0.insert([1, 64, 0], state("redstone_wire"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert_eq!(plan.to_destroy, vec![[1, 64, 0]]);
    }

    #[test]
    fn state_metadata_prevents_moving_an_uninitialized_chest_and_pops_water() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("chest"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
        grid.0.insert([1, 64, 0], state("water"));
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_destroy,
            vec![[1, 64, 0]]
        );
    }

    #[test]
    fn an_unextended_piston_can_move_but_an_extended_one_cannot() {
        let mut grid = Grid::default();
        let target = [1, 64, 0];
        grid.0.insert(
            target,
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap(),
        );
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move,
            vec![target]
        );
        grid.0.insert(
            target,
            blocks::state_id("piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn slime_moves_a_side_block_but_not_honey() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        grid.0.insert([1, 65, 0], state("stone"));
        grid.0.insert([1, 63, 0], state("honey_block"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert!(plan.to_move.contains(&[1, 65, 0]));
        assert!(!plan.to_move.contains(&[1, 63, 0]));
    }

    #[test]
    fn slime_does_not_drag_glazed_terracotta_or_immovable_side_blocks() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        grid.0.insert([1, 65, 0], state("white_glazed_terracotta"));
        grid.0.insert([1, 63, 0], state("obsidian"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert_eq!(plan.to_move, vec![[1, 64, 0]]);
    }

    #[test]
    fn sticky_branches_share_the_twelve_block_limit() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        for x in 2..=12 {
            grid.0.insert([x, 64, 0], state("stone"));
        }
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move
                .len(),
            12
        );
        grid.0.insert([1, 65, 0], state("stone"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn sticky_retraction_moves_the_block_into_the_head_cell() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("piston_head"));
        grid.0.insert([2, 64, 0], state("stone"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, false).unwrap();
        assert_eq!(plan.to_move, vec![[2, 64, 0]]);
    }

    #[test]
    fn unloaded_destination_rejects_the_entire_plan() {
        struct Boundary;
        impl PistonWorld for Boundary {
            fn block(&self, pos: [i32; 3]) -> Option<u16> {
                match pos[0] {
                    1 => Some(state("stone")),
                    _ => None,
                }
            }
            fn has_block_entity(&self, _: [i32; 3]) -> bool {
                false
            }
        }
        assert!(resolve(&Boundary, [0, 64, 0], Direction::East, true).is_none());
    }

    #[tokio::test]
    async fn region_refuses_a_push_into_an_unowned_loaded_chunk() {
        use std::sync::{Arc, Mutex};
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        for x in [0, 1] {
            loop {
                if let Some(result) = source.get(x, 0, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        let piston =
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap();
        let stone = state("stone");
        source.set_block(14, 70, 0, u32::from(piston)).unwrap();
        source.set_block(15, 70, 0, u32::from(stone)).unwrap();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let world = super::super::fluid::RegionFluidWorld::new(&source, &held, &tracking);
        assert!(operation(&world, [14, 70, 0], Direction::East, true).is_none());
        assert_eq!(source.block(14, 70, 0), Some(u32::from(piston)));
        assert_eq!(source.block(15, 70, 0), Some(u32::from(stone)));
    }

    #[test]
    fn world_border_includes_minimum_and_excludes_maximum() {
        let border = super::super::commands::WorldBorderState {
            center_x: 0.0,
            center_z: 0.0,
            diameter: 10.0,
            ..Default::default()
        };
        assert!(within_border(&border, [-5, 64, 0]));
        assert!(within_border(&border, [4, 64, 0]));
        assert!(!within_border(&border, [5, 64, 0]));
        assert!(!within_border(&border, [-6, 64, 0]));
    }

    #[test]
    fn extension_changes_move_a_line_and_create_a_head() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let piston =
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap();
        let stone = state("stone");
        grid.0.insert(base, piston);
        grid.0.insert([1, 64, 0], stone);
        let operation = operation(&grid, base, Direction::East, true).unwrap();
        assert_eq!(operation.entity_push_cells, vec![[1, 64, 0], [2, 64, 0]]);
        let plan = resolve(&grid, base, Direction::East, true).unwrap();
        for (pos, _, next) in changes(&grid, base, Direction::East, true, &plan).unwrap() {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([2, 64, 0]), Some(stone));
        assert_eq!(
            grid.block([1, 64, 0]),
            blocks::state_id(
                "piston_head",
                &[("facing", "east"), ("type", "normal"), ("short", "false")]
            )
        );
        assert_eq!(
            grid.block(base),
            blocks::state_id("piston", &[("facing", "east"), ("extended", "true")])
        );
    }

    #[test]
    fn retraction_removes_head_and_pulls_a_block() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let piston =
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap();
        let head =
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap();
        let stone = state("stone");
        grid.0.insert(base, piston);
        grid.0.insert([1, 64, 0], head);
        grid.0.insert([2, 64, 0], stone);
        let plan = resolve(&grid, base, Direction::East, false).unwrap();
        for (pos, _, next) in changes(&grid, base, Direction::East, false, &plan).unwrap() {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(stone));
        assert_eq!(grid.block([2, 64, 0]), Some(state("air")));
        assert_eq!(
            grid.block(base),
            blocks::state_id(
                "sticky_piston",
                &[("facing", "east"), ("extended", "false")]
            )
        );
    }

    #[test]
    fn sticky_retracts_even_when_its_target_cannot_be_pulled() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        grid.0.insert(
            base,
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        grid.0.insert(
            [1, 64, 0],
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap(),
        );
        grid.0.insert([2, 64, 0], state("obsidian"));
        let operation = operation(&grid, base, Direction::East, false).unwrap();
        for (pos, _, next) in operation.edits {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(state("air")));
        assert_eq!(grid.block([2, 64, 0]), Some(state("obsidian")));
    }

    #[test]
    fn a_plain_piston_leaves_the_block_behind_the_head_when_it_retracts() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        grid.0.insert(
            base,
            blocks::state_id("piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        grid.0.insert(
            [1, 64, 0],
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "normal")]).unwrap(),
        );
        grid.0.insert([2, 64, 0], state("stone"));
        let operation = operation(&grid, base, Direction::East, false).unwrap();
        for (pos, _, next) in operation.edits {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(state("air")));
        assert_eq!(grid.block([2, 64, 0]), Some(state("stone")));
    }

    #[test]
    fn a_single_block_with_air_behind_it_moves_one_cell() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("stone"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert_eq!(plan.to_move.len(), 1);
        assert!(plan.to_destroy.is_empty());
    }

    #[test]
    fn moving_block_progresses_and_completes_after_two_ticks() {
        let mut block = MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            state("stone"),
            Direction::East,
            true,
            false,
        );
        assert_eq!(block.progress, 0.0);
        block.tick();
        assert_eq!(block.progress, 0.5);
        assert!(!block.is_complete());
        block.tick();
        assert_eq!(block.progress, 1.0);
        assert!(block.is_complete());
    }

    #[test]
    fn moving_entity_nbt_uses_vanilla_fields_and_state_encoding() {
        let nbt = moving_entity_nbt(state("stone"), Direction::East, true, 0.0, true).unwrap();
        assert!(matches!(
            nbt.get("blockState"),
            Some(Tag::String(name)) if name == "minecraft:stone"
        ));
        assert!(matches!(nbt.get("facing"), Some(Tag::Byte(5))));
        assert!(matches!(nbt.get("progress"), Some(Tag::Float(progress)) if *progress == 0.0));
        assert!(matches!(nbt.get("extending"), Some(Tag::Byte(1))));
        assert!(matches!(nbt.get("source"), Some(Tag::Byte(1))));

        let stairs = blocks::state_id("oak_stairs", &[("facing", "east")]).unwrap();
        let Some(Tag::Compound(encoded)) = block_state_tag(stairs) else {
            panic!("non-default state must use the id/properties encoding");
        };
        assert!(matches!(
            encoded.get("id"),
            Some(Tag::String(name)) if name == "minecraft:oak_stairs"
        ));
        assert!(matches!(encoded.get("properties"), Some(Tag::Compound(_))));
    }

    #[test]
    fn moving_head_keeps_the_sticky_type() {
        let head = blocks::state_id(
            "piston_head",
            &[("facing", "east"), ("type", "sticky"), ("short", "false")],
        )
        .unwrap();
        let block = MovingBlock::new([0, 64, 0], [1, 64, 0], head, Direction::East, true, true);
        let state = block.moving_state().unwrap();
        let (name, properties) = blocks::state(state).unwrap();
        assert_eq!(name, "minecraft:moving_piston");
        assert!(properties.contains(&("facing", "east")));
        assert!(properties.contains(&("type", "sticky")));
        assert!(moving_state(Direction::East, false).is_some());
    }

    #[test]
    fn animations_advance_to_completion_without_duplicates() {
        let mut animations = Animations::default();
        let block = MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            state("stone"),
            Direction::East,
            true,
            false,
        );
        animations.start(vec![block], &HashSet::from([[1, 64, 0], [2, 64, 0]]));
        animations.advance();
        assert_eq!(animations.active().len(), 1);
        assert_eq!(animations.active()[0].progress, 0.5);
        animations.advance();
        assert!(animations.active().is_empty());
    }

    #[test]
    fn overlapping_animation_is_replaced() {
        let mut animations = Animations::default();
        let first = MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            state("stone"),
            Direction::East,
            true,
            false,
        );
        animations.start(vec![first], &HashSet::from([[2, 64, 0]]));
        let second = MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            state("stone"),
            Direction::East,
            false,
            false,
        );
        animations.start(vec![second.clone()], &HashSet::from([[1, 64, 0]]));
        assert_eq!(animations.active(), &[second]);
    }

    #[test]
    fn interrupted_extension_is_replaced_by_retraction() {
        let mut animations = Animations::default();
        let extension = MovingBlock::new(
            [0, 64, 0],
            [1, 64, 0],
            state("stone"),
            Direction::East,
            true,
            true,
        );
        animations.start(vec![extension], &HashSet::from([[0, 64, 0], [1, 64, 0]]));
        let retraction = MovingBlock::new(
            [2, 64, 0],
            [1, 64, 0],
            state("stone"),
            Direction::East,
            false,
            false,
        );
        animations.start(
            vec![retraction.clone()],
            &HashSet::from([[1, 64, 0], [2, 64, 0]]),
        );
        assert_eq!(animations.active(), &[retraction]);
    }

    #[test]
    fn extension_then_retraction_restores_the_world_state() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let sticky = blocks::state_id(
            "sticky_piston",
            &[("facing", "east"), ("extended", "false")],
        )
        .unwrap();
        let stone = state("stone");
        grid.0.insert(base, sticky);
        grid.0.insert([1, 64, 0], stone);

        let extension = operation(&grid, base, Direction::East, true).unwrap();
        assert_eq!(extension.head.unwrap().destination, [1, 64, 0]);
        assert!(
            extension
                .moves
                .iter()
                .any(|moved| moved.source == [1, 64, 0] && moved.destination == [2, 64, 0])
        );
        for &(pos, _, next) in &extension.edits {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }

        let retraction = operation(&grid, base, Direction::East, false).unwrap();
        assert!(
            retraction
                .moves
                .iter()
                .any(|moved| moved.source == [2, 64, 0] && moved.destination == [1, 64, 0])
        );
        for &(pos, _, next) in &retraction.edits {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(stone));
        assert_eq!(grid.block([2, 64, 0]), Some(state("air")));
        assert_eq!(
            grid.block(base),
            blocks::state_id(
                "sticky_piston",
                &[("facing", "east"), ("extended", "false")]
            )
        );
    }

    #[test]
    fn glazed_terracotta_is_pushed_but_never_pulled() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let glazed = state("white_glazed_terracotta");
        grid.0.insert([1, 64, 0], glazed);
        let extension = resolve(&grid, base, Direction::East, true).unwrap();
        assert_eq!(extension.to_move, vec![[1, 64, 0]]);

        grid.0.insert(
            base,
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        grid.0.insert(
            [1, 64, 0],
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap(),
        );
        grid.0.insert([2, 64, 0], glazed);
        let retraction = operation(&grid, base, Direction::East, false).unwrap();
        assert!(retraction.moves.is_empty());
        assert_eq!(grid.block([2, 64, 0]), Some(glazed));
    }

    #[test]
    fn retraction_requires_a_matching_piston_head() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        grid.0.insert(
            base,
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        grid.0.insert([2, 64, 0], state("stone"));
        assert!(operation(&grid, base, Direction::East, false).is_none());

        grid.0.insert(
            [1, 64, 0],
            blocks::state_id("piston_head", &[("facing", "west"), ("type", "sticky")]).unwrap(),
        );
        assert!(operation(&grid, base, Direction::East, false).is_none());
    }

    #[test]
    fn entity_space_clear_uses_collision_shapes() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("stone"));
        grid.0.insert([2, 64, 0], state("water"));
        let box_at = |x: f64, y: f64, z: f64| {
            ferrite_math::AABB::new(x + 0.2, y + 0.2, z + 0.2, x + 0.8, y + 1.8, z + 0.8)
        };
        assert!(!entity_space_clear(&grid, box_at(1.0, 64.0, 0.0)));
        assert!(entity_space_clear(&grid, box_at(2.0, 64.0, 0.0)));
        // Standing exactly on the stone is not an overlap.
        assert!(entity_space_clear(
            &grid,
            ferrite_math::AABB::new(1.2, 65.0, 0.2, 1.8, 66.8, 0.8)
        ));
    }

    #[test]
    fn entity_space_clear_rejects_unowned_cells() {
        struct Boundary;
        impl PistonWorld for Boundary {
            fn block(&self, pos: [i32; 3]) -> Option<u16> {
                (pos != [3, 64, 0]).then_some(state("air"))
            }
            fn has_block_entity(&self, _: [i32; 3]) -> bool {
                false
            }
        }
        let bbox = ferrite_math::AABB::new(3.2, 64.2, 0.2, 3.8, 65.8, 0.8);
        assert!(!entity_space_clear(&Boundary, bbox));
    }

    #[test]
    fn animations_split_and_merge_with_a_region() {
        let mut animations = Animations::default();
        let near = MovingBlock::new(
            [1, 64, 0],
            [2, 64, 0],
            state("stone"),
            Direction::East,
            true,
            false,
        );
        let far = MovingBlock::new(
            [20, 64, 0],
            [21, 64, 0],
            state("stone"),
            Direction::East,
            true,
            false,
        );
        animations.start(vec![near.clone(), far.clone()], &HashSet::new());
        let moved = animations.split(|pos| pos[0] < 16);
        assert_eq!(moved.active(), std::slice::from_ref(&near));
        assert_eq!(animations.active(), std::slice::from_ref(&far));
        animations.merge(moved);
        assert_eq!(animations.active().len(), 2);
        assert!(animations.active().contains(&near));
        assert!(animations.active().contains(&far));
    }

    #[test]
    fn a_sticky_line_stops_at_the_other_sticky_material() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        grid.0.insert([1, 65, 0], state("slime_block"));
        grid.0.insert([0, 65, 0], state("slime_block"));
        grid.0.insert([-1, 65, 0], state("honey_block"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert!(plan.to_move.contains(&[0, 65, 0]));
        assert!(plan.to_move.contains(&[1, 65, 0]));
        assert!(!plan.to_move.contains(&[-1, 65, 0]));
    }

    #[test]
    fn a_line_that_runs_into_a_queued_block_moves_before_it() {
        let grid = Grid::default();
        let mut resolver = Resolver {
            world: &grid,
            base: [0, 64, 0],
            extending: true,
            piston_facing: Direction::East,
            push_direction: Direction::East,
            head_cell: [1, 64, 0],
            air: state("air"),
            plan: Plan {
                to_move: vec![[1, 64, 0], [2, 64, 0], [3, 64, 0], [4, 64, 0]],
                to_destroy: Vec::new(),
            },
        };
        resolver.reorder_list_at_collision(2, 1);
        assert_eq!(
            resolver.plan.to_move,
            vec![[1, 64, 0], [3, 64, 0], [4, 64, 0], [2, 64, 0]]
        );
    }

    #[tokio::test]
    async fn a_sticky_side_branch_into_an_unowned_chunk_rejects_the_plan() {
        use std::sync::{Arc, Mutex};
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let piston =
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap();
        let slime = state("slime_block");
        let stone = state("stone");
        source.set_block(13, 70, 14, u32::from(piston)).unwrap();
        source.set_block(14, 70, 14, u32::from(slime)).unwrap();
        source.set_block(14, 70, 15, u32::from(stone)).unwrap();
        source.set_block(15, 70, 15, u32::from(stone)).unwrap();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let world = super::super::fluid::RegionFluidWorld::new(&source, &held, &tracking);
        assert!(operation(&world, [13, 70, 14], Direction::East, true).is_none());
    }

    #[tokio::test]
    async fn a_sticky_retraction_across_an_unowned_chunk_retracts_without_a_pull() {
        use std::sync::{Arc, Mutex};
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let sticky =
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap();
        let head =
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap();
        source.set_block(14, 70, 0, u32::from(sticky)).unwrap();
        source.set_block(15, 70, 0, u32::from(head)).unwrap();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let world = super::super::fluid::RegionFluidWorld::new(&source, &held, &tracking);
        let operation = operation(&world, [14, 70, 0], Direction::East, false).unwrap();
        assert!(operation.moves.is_empty());
        assert_eq!(
            operation
                .edits
                .iter()
                .map(|&(pos, _, _)| pos)
                .collect::<HashSet<_>>(),
            HashSet::from([[14, 70, 0], [15, 70, 0]])
        );
    }
}

/// Pins the behaviour behind Paper's `unsupported-settings` piston keys.
#[cfg(test)]
mod unsupported_settings_tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Grid(HashMap<[i32; 3], u16>);
    impl PistonWorld for Grid {
        fn block(&self, pos: [i32; 3]) -> Option<u16> {
            Some(*self.0.get(&pos).unwrap_or(&0))
        }
        fn has_block_entity(&self, _: [i32; 3]) -> bool {
            false
        }
    }
    fn state(name: &str) -> u16 {
        blocks::default_state(name).unwrap_or_else(|| panic!("no block {name}"))
    }
    fn base(facing: &str, extended: &str) -> u16 {
        blocks::state_id("piston", &[("facing", facing), ("extended", extended)]).unwrap()
    }
    fn head(facing: &str) -> u16 {
        blocks::state_id("piston_head", &[("facing", facing), ("type", "normal")]).unwrap()
    }

    #[test]
    fn removing_a_head_orphans_exactly_its_extended_base() {
        let mut grid = Grid::default();
        grid.0.insert([0, 64, 0], base("east", "true"));
        grid.0.insert([1, 64, 0], head("east"));
        let read = |pos| grid.block(pos);
        let air = state("air");
        assert_eq!(
            orphaned_base(read, [1, 64, 0], head("east"), air),
            Some([0, 64, 0])
        );
        // The same head replaced by a head of the same facing is not a removal.
        assert_eq!(
            orphaned_base(read, [1, 64, 0], head("east"), head("east")),
            None
        );
        // Other blocks are not heads.
        assert_eq!(orphaned_base(read, [1, 64, 0], state("stone"), air), None);
        // A retracted base, a base facing another way, or no base at all leaves nothing.
        grid.0.insert([0, 64, 0], base("east", "false"));
        assert_eq!(
            orphaned_base(|p| grid.block(p), [1, 64, 0], head("east"), air),
            None
        );
        grid.0.insert([0, 64, 0], base("north", "true"));
        assert_eq!(
            orphaned_base(|p| grid.block(p), [1, 64, 0], head("east"), air),
            None
        );
        grid.0.insert([0, 64, 0], state("stone"));
        assert_eq!(
            orphaned_base(|p| grid.block(p), [1, 64, 0], head("east"), air),
            None
        );
    }

    #[tokio::test]
    async fn a_broken_head_removes_the_base_unless_headless_pistons_are_allowed() {
        for allow in [false, true] {
            let source = crate::game::anvil::AnvilChunkSource::new_memory();
            source.prefetch(0, 0, 0);
            for _ in 0..2000 {
                if source.block(1, 64, 1).is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            assert!(source.block(1, 64, 1).is_some(), "chunk never loaded");
            let (base_state, head_state) = (base("east", "true"), head("east"));
            source.set_block(1, 64, 1, u32::from(base_state)).unwrap();
            source.set_block(2, 64, 1, u32::from(head_state)).unwrap();
            // The caller has already replaced the head with air.
            source.set_block(2, 64, 1, 0).unwrap();
            let removed =
                remove_orphaned_base(&source, [2, 64, 1], head_state, state("air"), allow);
            let left = source.block(1, 64, 1).unwrap();
            if allow {
                assert_eq!(removed, None);
                assert_eq!(left, u32::from(base_state), "headless piston is kept");
            } else {
                assert_eq!(removed, Some([1, 64, 1]));
                assert_eq!(left, 0, "no headless piston is left standing");
            }
        }
    }

    #[test]
    fn permanent_blocks_cannot_be_pushed_or_destroyed_by_a_piston() {
        for name in [
            "bedrock",
            "end_portal_frame",
            "end_portal",
            "end_gateway",
            "barrier",
            "command_block",
            "reinforced_deepslate",
        ] {
            let mut grid = Grid::default();
            // Directly in front of the piston.
            grid.0.insert([1, 64, 0], state(name));
            assert!(
                resolve(&grid, [0, 64, 0], Direction::East, true).is_none(),
                "{name} in front must stop the piston"
            );
            // Behind a pushed block: the push is refused, nothing is destroyed.
            grid.0.insert([1, 64, 0], state("stone"));
            grid.0.insert([2, 64, 0], state(name));
            assert!(
                resolve(&grid, [0, 64, 0], Direction::East, true).is_none(),
                "{name} behind a pushed block must stop the push"
            );
        }
    }

    #[test]
    fn tnt_carpets_and_rails_move_without_leaving_a_copy() {
        for name in ["tnt", "white_carpet", "rail"] {
            let mut grid = Grid::default();
            let moved = state(name);
            grid.0.insert([0, 64, 0], base("east", "false"));
            grid.0.insert([1, 64, 0], moved);
            let operation = operation(&grid, [0, 64, 0], Direction::East, true)
                .unwrap_or_else(|| panic!("{name} can be pushed"));
            for (pos, _, next) in operation.edits {
                grid.0.insert(pos, u16::try_from(next).unwrap());
            }
            let copies = grid.0.values().filter(|&&s| s == moved).count();
            assert_eq!(copies, 1, "{name}: pushing must not duplicate the block");
            assert_eq!(
                grid.block([2, 64, 0]),
                Some(moved),
                "{name} advanced one cell"
            );
        }
    }
}
