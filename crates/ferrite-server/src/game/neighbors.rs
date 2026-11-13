//! Region-local block updates after a block changes: vanilla's `Block.updateNeighbourShapes`,
//! `Level.neighborShapeChanged` and the `canSurvive` rules of blocks that need support.
//!
//! Vanilla applies a block change in `Level.setBlock` -> `markAndNotifyBlock`: every neighbour
//! gets `BlockState.updateShape` in `Block.UPDATE_SHAPE_ORDER` (west, east, down, up, north,
//! south), and a shape update that fails replaces the block with air. Ferrite only updates
//! blocks whose chunk this region holds (Folia: only the owning region may touch a chunk). A
//! change on a region border is handed to the neighbouring region as a `Signal::NeighborUpdate`
//! through [`NeighborWorld::notify_foreign`]; the owner queues it with [`Updates::remote_changed`]
//! and re-validates only its own blocks, so nothing is ever written across the boundary and the
//! two regions cannot bounce the same update back and forth.
//!
//! Support rules follow the vanilla block classes named on each branch. They are checked both
//! by shape updates and by `BlockItem.place` (`BlockPlaceContext.canPlace` -> `canSurvive`).

use ferrite_data::block_props;
use ferrite_math::Direction;
use std::collections::VecDeque;

/// Vanilla `Block.UPDATE_LIMIT`: how far one chain of updates may travel.
pub const UPDATE_LIMIT: usize = 512;

/// Vanilla `Block.UPDATE_SHAPE_ORDER`.
const SHAPE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];

/// Read access to block states.
pub trait BlockWorld {
    /// The state at a block, or `None` when its chunk is not loaded.
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16>;
}

/// A [`BlockWorld`] whose block states this framework may edit.
pub trait NeighborWorld: BlockWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16);
    /// Whether this region owns the chunk containing `(x, z)`.
    fn owns(&self, x: i32, z: i32) -> bool;
    /// A block at `origin` changed and `target`, one of its neighbours, belongs to another
    /// region. The implementation hands the update to the owner; the default drops it.
    fn notify_foreign(&mut self, _origin: [i32; 3], _target: [i32; 3]) {}
}

/// Face index in `ferrite_data::block_props::face_occlusion`: west, east, down, up, north, south.
const fn face_index(direction: Direction) -> usize {
    match direction {
        Direction::West => 0,
        Direction::East => 1,
        Direction::Down => 2,
        Direction::Up => 3,
        Direction::North => 4,
        Direction::South => 5,
    }
}

fn direction_of(name: &str) -> Option<Direction> {
    match name {
        "down" => Some(Direction::Down),
        "up" => Some(Direction::Up),
        "north" => Some(Direction::North),
        "south" => Some(Direction::South),
        "west" => Some(Direction::West),
        "east" => Some(Direction::East),
        _ => None,
    }
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn property(state: u16, key: &str) -> Option<&'static str> {
    let (_, props) = ferrite_data::blocks::state(state)?;
    props
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

fn block_name(state: u16) -> Option<&'static str> {
    ferrite_data::blocks::state(state).map(|(name, _)| short(name))
}

fn tagged(tag: &str, name: &str) -> bool {
    ferrite_data::tags::contains("block", tag, name)
}

fn neighbor(world: &impl BlockWorld, pos: [i32; 3], direction: Direction) -> Option<u16> {
    let (dx, dy, dz) = direction.step();
    world.block(pos[0] + dx, pos[1] + dy, pos[2] + dz)
}

/// Vanilla `BlockState.isFaceSturdy`: the neighbour in `direction` has a non-empty support shape
/// on the face pointing back at `pos`. An unloaded chunk counts as sturdy so a region border
/// never breaks another region's blocks.
fn face_sturdy(world: &impl BlockWorld, pos: [i32; 3], direction: Direction) -> bool {
    let Some(state) = neighbor(world, pos, direction) else {
        return true;
    };
    !block_props::face_occlusion(state, face_index(direction.opposite())).is_empty()
}

/// Vanilla `BushBlock.mayPlaceOn`: the block below is in `tag`.
fn may_place_on(world: &impl BlockWorld, pos: [i32; 3], tag: &str) -> bool {
    let Some(state) = neighbor(world, pos, Direction::Down) else {
        return true;
    };
    block_name(state).is_some_and(|name| tagged(tag, name))
}

/// Vanilla `WallTorchBlock.canSurvive` / `WallSignBlock.canSurvive`: the block behind the
/// horizontal facing holds the attachment.
fn wall_support(world: &impl BlockWorld, pos: [i32; 3], state: u16) -> bool {
    let Some(facing) = property(state, "facing").and_then(direction_of) else {
        return true;
    };
    face_sturdy(world, pos, facing.opposite())
}

/// Vanilla `TallFlowerBlock.canSurvive`: the upper half needs the lower half below it.
fn tall_half_survives(world: &impl BlockWorld, pos: [i32; 3], state: u16) -> bool {
    match property(state, "half") {
        Some("upper") => {
            let Some(below) = neighbor(world, pos, Direction::Down) else {
                return true;
            };
            block_name(below) == block_name(state)
        }
        _ => may_place_on(world, pos, "supports_vegetation"),
    }
}

/// Vanilla `canSurvive` of one block state at `pos`.
///
/// Unknown blocks survive: this framework only removes states whose vanilla class has an
/// explicit support rule.
pub fn can_survive(state: u16, pos: [i32; 3], world: &impl BlockWorld) -> bool {
    let Some(name) = block_name(state) else {
        return true;
    };
    // TorchBlock / WallTorchBlock, including soul, redstone and copper variants.
    if name.ends_with("wall_torch") {
        return wall_support(world, pos, state);
    }
    if name == "torch" || name.ends_with("_torch") {
        return face_sturdy(world, pos, Direction::Down);
    }
    // LanternBlock: hanging lanterns hang from a ceiling, the rest stand on a floor. Only the
    // lanterns have the `hanging` property: `sea_lantern` and `jack_o_lantern` are full blocks.
    if name.ends_with("lantern") && property(state, "hanging").is_some() {
        return if property(state, "hanging") == Some("true") {
            face_sturdy(world, pos, Direction::Up)
        } else {
            face_sturdy(world, pos, Direction::Down)
        };
    }
    // DoorBlock: the upper half belongs to the lower half; the lower half needs solid ground.
    if tagged("doors", name) {
        return if property(state, "half") == Some("upper") {
            neighbor(world, pos, Direction::Down)
                .and_then(block_name)
                .is_some_and(|below| below == name)
        } else {
            face_sturdy(world, pos, Direction::Down)
        };
    }
    // TallFlowerBlock (sunflower, lilac, rose bush, peony).
    if matches!(name, "sunflower" | "lilac" | "rose_bush" | "peony") {
        return tall_half_survives(world, pos, state);
    }
    // FlowerBlock / SaplingBlock (BushBlock.mayPlaceOn).
    if tagged("small_flowers", name) || tagged("saplings", name) {
        return may_place_on(world, pos, "supports_vegetation");
    }
    // CropBlock.mayPlaceOn: only farmland supports crops.
    if tagged("crops", name) {
        return may_place_on(world, pos, "supports_crops");
    }
    // BaseRailBlock.canSurvive.
    if tagged("rails", name) {
        return face_sturdy(world, pos, Direction::Down);
    }
    // StandingSignBlock.canSurvive.
    if tagged("standing_signs", name) {
        return face_sturdy(world, pos, Direction::Down);
    }
    // WallSignBlock.canSurvive.
    if tagged("wall_signs", name) {
        return wall_support(world, pos, state);
    }
    // ButtonBlock.canSurvive, by its attach face.
    if tagged("buttons", name) {
        return match property(state, "face") {
            Some("ceiling") => face_sturdy(world, pos, Direction::Up),
            Some("wall") => wall_support(world, pos, state),
            _ => face_sturdy(world, pos, Direction::Down),
        };
    }
    // BasePressurePlateBlock.canSurvive.
    if tagged("pressure_plates", name) {
        return face_sturdy(world, pos, Direction::Down);
    }
    // RedStoneWireBlock.canSurvive.
    if name == "redstone_wire" {
        return face_sturdy(world, pos, Direction::Down);
    }
    true
}

/// Pending block changes: each origin notifies its neighbours, and a removed block becomes a new
/// origin, so one break cascades until nothing is left unsupported.
#[derive(Debug, Default)]
pub struct Updates {
    /// `(origin, remote)`: a remote origin was changed by another region, so its unowned
    /// neighbours are not forwarded again.
    queue: VecDeque<([i32; 3], bool)>,
}

impl Updates {
    /// Queues the block at `pos` as changed (vanilla `Level.markAndNotifyBlock`).
    pub fn block_changed(&mut self, pos: [i32; 3]) {
        self.queue.push_back((pos, false));
    }

    /// Queues a change another region made at `pos`: only this region's neighbours of it are
    /// re-validated, and nothing is forwarded back.
    pub fn remote_changed(&mut self, pos: [i32; 3]) {
        self.queue.push_back((pos, true));
    }

    /// Flushes the queue. Returns the blocks that no longer survive, with their old states;
    /// the caller drops their loot. Neighbours in chunks this region does not own are never
    /// written: a local change is handed to their owner through [`NeighborWorld::notify_foreign`].
    pub fn apply(&mut self, world: &mut impl NeighborWorld, removed: &mut Vec<([i32; 3], u16)>) {
        let air = ferrite_data::blocks::default_state("air").expect("vanilla has an air state");
        let mut budget = UPDATE_LIMIT;
        while let Some((pos, remote)) = self.queue.pop_front() {
            if budget == 0 {
                self.queue.clear();
                break;
            }
            budget -= 1;
            for direction in SHAPE_ORDER {
                let (dx, dy, dz) = direction.step();
                let target = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                if !world.owns(target[0], target[2]) {
                    if !remote && world.owns(pos[0], pos[2]) {
                        world.notify_foreign(pos, target);
                    }
                    continue;
                }
                let Some(state) = world.block(target[0], target[1], target[2]) else {
                    continue;
                };
                if can_survive(state, target, world) {
                    continue;
                }
                // Vanilla `Level.neighborShapeChanged` replaces the state with air; the item
                // drop players observe comes from `Block.dropResources` for that state.
                world.set_block(target[0], target[1], target[2], air);
                removed.push((target, state));
                self.queue.push_back((target, false));
            }
        }
    }
}

impl BlockWorld for super::anvil::AnvilChunkSource {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        super::anvil::AnvilChunkSource::block(self, x, y, z)
            .and_then(|state| u16::try_from(state).ok())
    }
}

impl BlockWorld for super::fluid::RegionFluidWorld<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        u16::try_from(ferrite_fluid::FluidWorld::block(self, x, y, z)).ok()
    }
}

impl NeighborWorld for super::fluid::RegionFluidWorld<'_> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
        ferrite_fluid::FluidWorld::set_block(self, x, y, z, u32::from(state));
    }

    fn owns(&self, x: i32, z: i32) -> bool {
        super::fluid::RegionFluidWorld::owns(self, x, z)
    }

    fn notify_foreign(&mut self, origin: [i32; 3], target: [i32; 3]) {
        // Only a loaded chunk has an owner that can act; an unloaded one re-validates on load.
        if let Some(signals) = &self.signals
            && self.source.block(target[0], target[1], target[2]).is_some()
        {
            let _ = signals.send(super::region::Signal::NeighborUpdate {
                dimension: self.source.dimension(),
                origin,
                target,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Grid {
        blocks: HashMap<[i32; 3], u16>,
        owned: Vec<[i32; 3]>,
    }

    impl Grid {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
                owned: Vec::new(),
            }
        }

        fn set(&mut self, pos: [i32; 3], name: &str) -> u16 {
            self.set_state(pos, state(name))
        }

        fn set_state(&mut self, pos: [i32; 3], state: u16) -> u16 {
            self.blocks.insert(pos, state);
            self.owned.push(pos);
            state
        }

        fn get(&self, pos: [i32; 3]) -> u16 {
            *self.blocks.get(&pos).unwrap_or(&state("air"))
        }
    }

    fn state(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).unwrap()
    }

    fn state_with(name: &str, props: &[(&str, &str)]) -> u16 {
        ferrite_data::blocks::state_id(name, props).unwrap()
    }

    impl BlockWorld for Grid {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            Some(self.get([x, y, z]))
        }
    }

    impl NeighborWorld for Grid {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
            self.blocks.insert([x, y, z], state);
        }

        fn owns(&self, _: i32, _: i32) -> bool {
            true
        }
    }

    #[test]
    fn standing_and_wall_torches_need_their_support() {
        let mut grid = Grid::new();
        let floor = grid.set([0, 63, 0], "stone");
        let torch = grid.set([0, 64, 0], "torch");
        assert!(can_survive(torch, [0, 64, 0], &grid));
        // A wall torch facing east hangs on the block to its west.
        let wall_pos = [2, 64, 0];
        let wall = state_with("wall_torch", &[("facing", "east")]);
        grid.set_state([1, 64, 0], floor);
        assert!(can_survive(wall, wall_pos, &grid));
        grid.set_state([1, 64, 0], state("air"));
        assert!(!can_survive(wall, wall_pos, &grid));
        // The floor turns into air: the standing torch no longer survives.
        grid.set([0, 63, 0], "air");
        assert!(!can_survive(torch, [0, 64, 0], &grid));
    }

    #[test]
    fn flowers_need_dirt_or_farmland_but_doors_and_rails_need_any_sturdy_face() {
        let mut grid = Grid::new();
        let dirt = grid.set([0, 63, 0], "dirt");
        let poppy = grid.set([0, 64, 0], "poppy");
        assert!(can_survive(poppy, [0, 64, 0], &grid));
        grid.set_state([0, 63, 0], state("stone"));
        assert!(!can_survive(poppy, [0, 64, 0], &grid));
        grid.set_state([0, 63, 0], state("farmland"));
        assert!(can_survive(poppy, [0, 64, 0], &grid));

        grid.set_state([0, 63, 0], state("stone"));
        let door = state_with("oak_door", &[("half", "lower")]);
        let upper = state_with("oak_door", &[("half", "upper")]);
        assert!(can_survive(door, [0, 64, 0], &grid));
        assert!(!can_survive(upper, [0, 65, 0], &grid));
        grid.set_state([0, 64, 0], door);
        assert!(can_survive(upper, [0, 65, 0], &grid));

        let rail = state("rail");
        grid.set_state([0, 63, 0], state("stone"));
        assert!(can_survive(rail, [0, 64, 0], &grid));
        grid.set_state([0, 63, 0], state("air"));
        assert!(!can_survive(rail, [0, 64, 0], &grid));
        assert!(can_survive(dirt, [0, 63, 0], &grid));
    }

    #[test]
    fn unsupported_blocks_break_with_their_state_and_cascade() {
        let mut grid = Grid::new();
        grid.set([0, 62, 0], "stone");
        grid.set([0, 63, 0], "dirt");
        let torch = grid.set([0, 64, 0], "torch");
        assert!(can_survive(torch, [0, 64, 0], &grid));

        // Breaking the dirt under the torch removes the torch in one propagation.
        grid.set([0, 63, 0], "air");
        let mut updates = Updates::default();
        updates.block_changed([0, 63, 0]);
        let mut removed = Vec::new();
        updates.apply(&mut grid, &mut removed);
        assert_eq!(removed, vec![([0, 64, 0], torch)]);
        assert_eq!(grid.get([0, 64, 0]), state("air"));
    }

    /// Two regions over one block map: `west` owns x < 16, the other owns x >= 16.
    struct Border {
        grid: Grid,
        west: bool,
        sent: Vec<([i32; 3], [i32; 3])>,
    }
    impl BlockWorld for Border {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            self.grid.block(x, y, z)
        }
    }
    impl NeighborWorld for Border {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u16) {
            assert!(self.owns(x, z), "a region wrote a block it does not own");
            self.grid.set_block(x, y, z, state);
        }
        fn owns(&self, x: i32, _: i32) -> bool {
            (x.div_euclid(16) < 1) == self.west
        }
        fn notify_foreign(&mut self, origin: [i32; 3], target: [i32; 3]) {
            self.sent.push((origin, target));
        }
    }

    /// A wall torch on the east side of the border hangs on the stone block west of it.
    fn border_with_wall_torch(west: bool) -> (Border, u16) {
        let mut grid = Grid::new();
        grid.set([15, 64, 0], "stone");
        let wall = state_with("wall_torch", &[("facing", "east")]);
        grid.set_state([16, 64, 0], wall);
        (
            Border {
                grid,
                west,
                sent: Vec::new(),
            },
            wall,
        )
    }

    /// Same-region baseline: both blocks are owned, so the torch breaks at once.
    #[test]
    fn a_wall_torch_breaks_when_its_support_goes_in_the_same_region() {
        let mut grid = Grid::new();
        grid.set([15, 64, 0], "stone");
        let wall = state_with("wall_torch", &[("facing", "east")]);
        grid.set_state([16, 64, 0], wall);
        grid.set([15, 64, 0], "air");
        let mut updates = Updates::default();
        updates.block_changed([15, 64, 0]);
        let mut removed = Vec::new();
        updates.apply(&mut grid, &mut removed);
        assert_eq!(removed, vec![([16, 64, 0], wall)]);
    }

    /// Cross-region: the support breaks in the west region. It must not write the east block,
    /// and it must hand the change to the east region, which then removes the torch.
    #[test]
    fn a_change_on_a_region_border_is_handed_to_the_owner_not_ignored() {
        let (mut west, wall) = border_with_wall_torch(true);
        west.grid.blocks.insert([15, 64, 0], state("air"));
        let mut updates = Updates::default();
        updates.block_changed([15, 64, 0]);
        let mut removed = Vec::new();
        updates.apply(&mut west, &mut removed);
        assert!(
            removed.is_empty(),
            "the west region must not break an east block"
        );
        assert_eq!(west.grid.get([16, 64, 0]), wall);
        assert_eq!(west.sent, vec![([15, 64, 0], [16, 64, 0])]);

        // The east region receives the origin as a remote change.
        let mut east = Border {
            grid: west.grid,
            west: false,
            sent: Vec::new(),
        };
        let mut updates = Updates::default();
        updates.remote_changed([15, 64, 0]);
        updates.apply(&mut east, &mut removed);
        assert_eq!(removed, vec![([16, 64, 0], wall)]);
        assert_eq!(east.grid.get([16, 64, 0]), state("air"));
        // The remote origin itself is not forwarded again; only the torch removal, a new local
        // change, is announced to the west region.
        assert_eq!(east.sent, vec![([16, 64, 0], [15, 64, 0])]);
    }

    /// A removal that cascades out of a remote change is a local change: it is forwarded on.
    #[test]
    fn a_cascade_after_a_remote_change_is_forwarded_to_the_next_owner() {
        let (mut east, wall) = border_with_wall_torch(false);
        east.grid.blocks.insert([15, 64, 0], state("air"));
        let mut updates = Updates::default();
        updates.remote_changed([15, 64, 0]);
        let mut removed = Vec::new();
        updates.apply(&mut east, &mut removed);
        assert_eq!(removed, vec![([16, 64, 0], wall)]);
        // The removed torch is a local change whose west neighbour belongs to the other region.
        assert_eq!(east.sent, vec![([16, 64, 0], [15, 64, 0])]);
    }

    #[test]
    fn a_remote_change_never_touches_blocks_beyond_the_owned_side() {
        let (mut west, wall) = border_with_wall_torch(true);
        let mut updates = Updates::default();
        updates.remote_changed([16, 63, 0]);
        let mut removed = Vec::new();
        updates.apply(&mut west, &mut removed);
        assert!(removed.is_empty());
        assert_eq!(west.grid.get([16, 64, 0]), wall);
        assert!(west.sent.is_empty());
    }

    #[test]
    fn full_blocks_named_lantern_need_no_support() {
        // Regression: `ends_with("lantern")` matched `sea_lantern` and `jack_o_lantern`, so a
        // block update next to one with nothing underneath turned it into air.
        let grid = Grid::new();
        for name in ["sea_lantern", "jack_o_lantern"] {
            assert!(can_survive(state(name), [0, 5, 0], &grid), "{name}");
        }
        // A real lantern still needs its floor, or its ceiling when it hangs.
        assert!(!can_survive(state("lantern"), [0, 5, 0], &grid));
        let hanging = state_with("lantern", &[("hanging", "true")]);
        assert!(!can_survive(hanging, [0, 5, 0], &grid));
    }
}
