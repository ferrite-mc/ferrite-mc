//! Region-local redstone, following the vanilla block classes:
//! `RedStoneWireBlock`, `LeverBlock`, `ButtonBlock`, `PressurePlateBlock`,
//! `RedstoneTorchBlock`/`RedstoneWallTorchBlock`, `RedstoneBlock`, `ObserverBlock`,
//! `TargetBlock`, `DaylightDetectorBlock`, `DiodeBlock` (`RepeaterBlock`) and
//! `ComparatorBlock`, plus the blocks they drive (`RedstoneLampBlock`, `DoorBlock`,
//! `TrapDoorBlock`, `FenceGateBlock`, `NoteBlock`, `TntBlock`, `PoweredRailBlock`).
//!
//! Like the neighbour and fluid systems, only chunks this region holds may be read or
//! written. Signal propagation mirrors vanilla's two notions of power:
//!
//! - weak power (`BlockState.getSignal`) reaches a directly adjacent wire or consumer;
//! - strong power (`BlockState.getDirectSignal`) also travels through one conductive block,
//!   which is why a torch attached to a stone block powers wire on top of that stone.
//!
//! Wire networks are recomputed as a connected component (bounded by [`WIRE_LIMIT`]) with a
//! highest-power-first relaxation, so removing a source anywhere in a loop settles instead of
//! keeping a stale value. Delayed components use the region's own scheduled block ticks
//! (`Level.scheduleTick`), advanced once per region tick.

use super::{fluid::RegionFluidWorld, region::Signal};
use ferrite_data::blocks;
use ferrite_math::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

/// Vanilla `Block.UPDATE_LIMIT` scale for one region tick's redstone work.
const UPDATE_LIMIT: usize = 4096;
/// Longest wire network recomputed from scratch; bigger networks update incrementally.
const WIRE_LIMIT: usize = 4096;
/// Vanilla `RedstoneTorchBlock.RECENT_TOGGLE_TIMER` and `MAX_RECENT_TOGGLES`.
const TORCH_TOGGLE_WINDOW: u64 = 60;
const TORCH_MAX_TOGGLES: usize = 8;
/// `scheduleTick(pos, block, 160)` after a burnout.
const TORCH_BURNOUT_TICKS: u64 = 160;
/// Vanilla button press lengths: stone 1 s, wooden 1.5 s.
const STONE_BUTTON_TICKS: u64 = 20;
const WOOD_BUTTON_TICKS: u64 = 30;

const POWER: [&str; 16] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
];
/// A cross-region wake: the block to re-evaluate (in the other region) and the block here
/// whose change caused it. Blocks that watch a face (observers) need to know which one.
pub type Wake = ([i32; 3], [i32; 3]);

/// Block access for the engine. `block` returns `None` for chunks this region does not own,
/// so a signal never leaks into another region's territory (Folia ownership).
pub trait RedstoneWorld {
    fn block(&self, pos: [i32; 3]) -> Option<u16>;
    fn set_block(&mut self, pos: [i32; 3], state: u16);
    fn brightness(&self, pos: [i32; 3]) -> u8;

    /// Read a block across a region boundary for power computation. Unlike [`Self::block`],
    /// this may see a loaded chunk another region owns. The read is never accompanied by a
    /// write, so a network still stops at the boundary.
    fn signal_block(&self, pos: [i32; 3]) -> Option<u16> {
        self.block(pos)
    }

    /// Ask the regions owning the `Wake` targets to re-evaluate redstone there. The default is a
    /// no-op for local worlds and tests.
    fn wake_redstone(&mut self, _wakes: &[Wake]) {}

    /// Vanilla `BlockState.getAnalogOutputSignal` for a container (`hasAnalogOutputSignal`),
    /// which a comparator compares instead of the block's weak power.
    fn container_signal(&self, _pos: [i32; 3]) -> Option<u8> {
        None
    }
}

impl RedstoneWorld for RegionFluidWorld<'_> {
    fn block(&self, pos: [i32; 3]) -> Option<u16> {
        if !self.owns(pos[0], pos[2]) {
            return None;
        }
        self.source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
    }

    fn set_block(&mut self, pos: [i32; 3], state: u16) {
        ferrite_fluid::FluidWorld::set_block(self, pos[0], pos[1], pos[2], u32::from(state));
    }

    fn brightness(&self, pos: [i32; 3]) -> u8 {
        RegionFluidWorld::brightness(self, pos[0], pos[1], pos[2])
    }

    fn signal_block(&self, pos: [i32; 3]) -> Option<u16> {
        self.source
            .block(pos[0], pos[1], pos[2])
            .and_then(|state| u16::try_from(state).ok())
    }

    fn wake_redstone(&mut self, wakes: &[Wake]) {
        let Some(signals) = &self.signals else {
            return;
        };
        let wakes: Vec<_> = wakes
            .iter()
            .copied()
            .filter(|(pos, _)| self.source.block(pos[0], pos[1], pos[2]).is_some())
            .collect();
        if !wakes.is_empty() {
            let _ = signals.send(Signal::RedstoneChange {
                dimension: self.source.dimension(),
                positions: wakes,
            });
        }
    }

    fn container_signal(&self, pos: [i32; 3]) -> Option<u8> {
        let state = self.signal_block(pos)?;
        let (name, _) = blocks::state(state)?;
        let size = super::block_container::inventory_slots(name)?;
        let items = super::block_container::load_slots(self.source, pos, size);
        Some(super::hopper::comparator_signal(&items))
    }
}

fn name(state: u16) -> Option<&'static str> {
    blocks::state(state).map(|(name, _)| name.strip_prefix("minecraft:").unwrap_or(name))
}

fn property(state: u16, key: &str) -> Option<&'static str> {
    let (_, props) = blocks::state(state)?;
    props
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

fn property_bool(state: u16, key: &str) -> bool {
    property(state, key) == Some("true")
}

fn property_u8(state: u16, key: &str) -> Option<u8> {
    property(state, key)?.parse().ok()
}

/// The same block state with some properties replaced (`BlockState.setValue`).
fn with_properties(state: u16, changes: &[(&str, &str)]) -> Option<u16> {
    let (name, props) = blocks::state(state)?;
    let props: Vec<(&str, &str)> = props
        .iter()
        .map(|&(key, value)| {
            let value = changes
                .iter()
                .find(|(changed, _)| *changed == key)
                .map_or(value, |(_, value)| *value);
            (key, value)
        })
        .collect();
    blocks::state_id(name, &props)
}

/// Toggle a boolean property (`LeverBlock.use`).
fn toggle(state: u16, key: &str) -> Option<u16> {
    let value = if property_bool(state, key) {
        "false"
    } else {
        "true"
    };
    with_properties(state, &[(key, value)])
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

fn facing(state: u16) -> Option<Direction> {
    direction_of(property(state, "facing")?)
}

fn offset(pos: [i32; 3], direction: Direction) -> [i32; 3] {
    let (dx, dy, dz) = direction.step();
    [pos[0] + dx, pos[1] + dy, pos[2] + dz]
}

fn is_wire(state: u16) -> bool {
    name(state) == Some("redstone_wire")
}

fn is_button(state: u16) -> bool {
    name(state).is_some_and(|name| name.ends_with("_button"))
}

fn is_pressure_plate(state: u16) -> bool {
    name(state).is_some_and(|name| name.ends_with("_pressure_plate"))
}

fn is_repeater(state: u16) -> bool {
    name(state) == Some("repeater")
}

fn is_comparator(state: u16) -> bool {
    name(state) == Some("comparator")
}

fn is_diode(state: u16) -> bool {
    is_repeater(state) || is_comparator(state)
}

fn is_observer(state: u16) -> bool {
    name(state) == Some("observer")
}

fn is_torch(state: u16) -> bool {
    matches!(name(state), Some("redstone_torch" | "redstone_wall_torch"))
}

/// `NeighborUpdater.UPDATE_ORDER`: the order a changed block notifies its neighbours in, which
/// differs from `Direction.values()` and decides which of two reacting blocks updates first.
const UPDATE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn clockwise(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        other => other,
    }
}

fn counter_clockwise(direction: Direction) -> Direction {
    clockwise(direction).opposite()
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

/// Vanilla `BlockState.isRedstoneConductor`: a full block that carries strong power. Redstone
/// components, pistons, observers and the like are excluded by the generated block data.
fn is_conductor(state: u16) -> bool {
    ferrite_data::block_props::get(state).is_redstone_conductor
}

/// Vanilla `BlockState.isSignalSource`: what a wire connects to sideways.
fn is_signal_source(state: u16) -> bool {
    name(state).is_some_and(|name| {
        matches!(
            name,
            "redstone_wire"
                | "lever"
                | "redstone_torch"
                | "redstone_wall_torch"
                | "redstone_block"
                | "repeater"
                | "comparator"
                | "observer"
                | "target"
                | "daylight_detector"
                | "lightning_rod"
                | "tripwire_hook"
                | "detector_rail"
                | "trapped_chest"
                | "lectern"
                | "sculk_sensor"
                | "calibrated_sculk_sensor"
        ) || name.ends_with("_button")
            || name.ends_with("_pressure_plate")
    })
}

/// `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`: the way from the block
/// the lever or button sits on towards the lever.
fn connected_direction(state: u16) -> Option<Direction> {
    match property(state, "face") {
        Some("floor") => Some(Direction::Up),
        Some("ceiling") => Some(Direction::Down),
        _ => facing(state),
    }
}

fn connection_key(direction: Direction) -> &'static str {
    match direction {
        Direction::North => "north",
        Direction::East => "east",
        Direction::South => "south",
        _ => "west",
    }
}

/// Weak signal `state` emits towards a consumer standing at the other end of `toward`
/// (`BlockState.getSignal(level, pos, toward)`: `toward` points from the consumer to the
/// source). `output` is a comparator's computed strength. `wires` is false while a wire
/// network computes its own targets (`RedStoneWireBlock.shouldSignal`).
fn signal_toward(state: u16, output: u8, toward: Direction, wires: bool) -> u8 {
    let Some(block) = name(state) else {
        return 0;
    };
    let lit = |on: bool| if on { 15 } else { 0 };
    match block {
        "redstone_block" => 15,
        "redstone_torch" => lit(property_bool(state, "lit") && toward != Direction::Up),
        "redstone_wall_torch" => lit(property_bool(state, "lit") && facing(state) != Some(toward)),
        "lever" | "detector_rail" | "tripwire_hook" => lit(property_bool(state, "powered")),
        // `TrappedChestBlock.getSignal`: the number of viewers, clamped to 0..=15.
        "trapped_chest" => output.min(15),
        "observer" | "repeater" => {
            lit(property_bool(state, "powered") && facing(state) == Some(toward))
        }
        "comparator" => {
            if property_bool(state, "powered") && facing(state) == Some(toward) {
                output
            } else {
                0
            }
        }
        "target"
        | "daylight_detector"
        | "light_weighted_pressure_plate"
        | "heavy_weighted_pressure_plate" => property_u8(state, "power").unwrap_or(0),
        "redstone_wire" => {
            if !wires || toward == Direction::Down {
                return 0;
            }
            let power = property_u8(state, "power").unwrap_or(0);
            let connected = toward != Direction::Up
                && property(state, connection_key(toward.opposite()))
                    .is_some_and(|value| value != "none");
            if toward == Direction::Up || connected {
                power
            } else {
                0
            }
        }
        name if name.ends_with("_button") || name.ends_with("_pressure_plate") => {
            lit(property_bool(state, "powered"))
        }
        _ => 0,
    }
}

/// Strong signal towards `toward` (`BlockState.getDirectSignal`): only what a conductive
/// block can pass on.
fn direct_toward(state: u16, output: u8, toward: Direction, wires: bool) -> u8 {
    let Some(block) = name(state) else {
        return 0;
    };
    match block {
        "redstone_torch" | "redstone_wall_torch" if toward == Direction::Down => {
            signal_toward(state, output, toward, wires)
        }
        "lever" => {
            if property_bool(state, "powered") && connected_direction(state) == Some(toward) {
                15
            } else {
                0
            }
        }
        name if name.ends_with("_button") => {
            if property_bool(state, "powered") && connected_direction(state) == Some(toward) {
                15
            } else {
                0
            }
        }
        // `DetectorRailBlock.getDirectSignal`: powered rails only push upwards.
        "detector_rail" if toward == Direction::Up => signal_toward(state, output, toward, wires),
        // `TripWireHookBlock.getDirectSignal`: powered and facing `toward`.
        "tripwire_hook" if facing(state) == Some(toward) => {
            signal_toward(state, output, toward, wires)
        }
        // `TrappedChestBlock.getDirectSignal`: only upwards.
        "trapped_chest" if toward == Direction::Up => signal_toward(state, output, toward, wires),
        name if name.ends_with("_pressure_plate") && toward == Direction::Up => {
            signal_toward(state, output, toward, wires)
        }
        "repeater" | "observer" | "comparator" | "redstone_wire" => {
            signal_toward(state, output, toward, wires)
        }
        _ => 0,
    }
}

/// `BlockState.getAnalogOutputSignal` for blocks whose level is a property.
fn analog_property_signal(state: u16) -> Option<u8> {
    match name(state)? {
        "composter" => property_u8(state, "level"),
        "cake" => property_u8(state, "bites").map(|bites| 7u8.saturating_sub(bites) * 2),
        "end_portal_frame" => Some(if property_bool(state, "eye") { 15 } else { 0 }),
        "respawn_anchor" => {
            property_u8(state, "charges").map(|c| (f32::from(c) / 4.0 * 15.0) as u8)
        }
        "beehive" | "bee_nest" => property_u8(state, "honey_level"),
        "cauldron" => Some(0),
        "lava_cauldron" => Some(3),
        "water_cauldron" | "powder_snow_cauldron" => property_u8(state, "level"),
        _ => None,
    }
}

/// `analog` also carries item-frame strengths, under a key that cannot collide with a block
/// position (y is shifted by a million per direction). One frame per block face matters:
/// `ComparatorBlock.getItemFrame` accepts exactly one frame facing the comparator's direction.
const FRAME_BIAS: i32 = 1_000_000;

fn frame_key(pos: [i32; 3], facing: Direction) -> [i32; 3] {
    let index = Direction::ALL
        .iter()
        .position(|&d| d == facing)
        .map_or(0, |i| i as i32 + 1);
    [pos[0], pos[1].saturating_add(FRAME_BIAS * index), pos[2]]
}

/// Reads power the way `Level`/`SignalGetter` does, over blocks this region may read.
struct Probe<'a, W: RedstoneWorld> {
    world: &'a W,
    analog: &'a HashMap<[i32; 3], u8>,
    /// Whether wires emit (false while a wire network computes its own target).
    wires: bool,
}

impl<'a, W: RedstoneWorld> Probe<'a, W> {
    fn new(world: &'a W, analog: &'a HashMap<[i32; 3], u8>) -> Self {
        Self {
            world,
            analog,
            wires: true,
        }
    }

    fn output(&self, pos: [i32; 3]) -> u8 {
        self.analog.get(&pos).copied().unwrap_or(0)
    }

    /// `Level.getDirectSignal(pos, toward)`.
    fn direct(&self, pos: [i32; 3], toward: Direction) -> u8 {
        self.world.signal_block(pos).map_or(0, |state| {
            direct_toward(state, self.output(pos), toward, self.wires)
        })
    }

    /// `Level.getDirectSignalTo(pos)`: the strongest direct signal reaching a conductor.
    fn direct_to(&self, pos: [i32; 3]) -> u8 {
        Direction::ALL
            .iter()
            .map(|&direction| self.direct(offset(pos, direction), direction))
            .max()
            .unwrap_or(0)
    }

    /// `Level.getSignal(pos, toward)`: a conductor also passes on the strong power it carries.
    fn signal(&self, pos: [i32; 3], toward: Direction) -> u8 {
        let Some(state) = self.world.signal_block(pos) else {
            return 0;
        };
        let weak = signal_toward(state, self.output(pos), toward, self.wires);
        if is_conductor(state) {
            weak.max(self.direct_to(pos))
        } else {
            weak
        }
    }

    /// `Level.getBestNeighborSignal` (also what `hasNeighborSignal` tests for non-zero).
    fn neighbor_signal(&self, pos: [i32; 3]) -> u8 {
        Direction::ALL
            .iter()
            .map(|&direction| self.signal(offset(pos, direction), direction))
            .max()
            .unwrap_or(0)
    }

    /// `DiodeBlock.getInputSignal`: the block behind a repeater or comparator, wires included.
    fn diode_input(&self, pos: [i32; 3], facing: Direction) -> u8 {
        let behind = offset(pos, facing);
        let power = self.signal(behind, facing);
        if power >= 15 {
            return power;
        }
        match self.world.signal_block(behind) {
            Some(state) if is_wire(state) => power.max(property_u8(state, "power").unwrap_or(0)),
            _ => power,
        }
    }

    /// Strength a block reports to a comparator (`hasAnalogOutputSignal`).
    fn analog_block(&self, pos: [i32; 3], state: u16) -> Option<u8> {
        analog_property_signal(state).or_else(|| self.world.container_signal(pos))
    }

    /// `ComparatorBlock.getInputSignal`: containers and other analog blocks directly behind,
    /// or one block further behind through a conductor.
    fn comparator_input(&self, pos: [i32; 3], facing: Direction) -> u8 {
        let mut input = self.diode_input(pos, facing);
        let behind = offset(pos, facing);
        let Some(state) = self.world.signal_block(behind) else {
            return input;
        };
        if let Some(analog) = self.analog_block(behind, state) {
            input = analog;
        } else if input < 15 && is_conductor(state) {
            let far = offset(behind, facing);
            if let Some(far_state) = self.world.signal_block(far) {
                // `Math.max(block analog, item frame analog)`; either one alone counts.
                let block = self.analog_block(far, far_state);
                let frame = self.analog.get(&frame_key(far, facing)).copied();
                if let Some(best) = block.into_iter().chain(frame).max() {
                    input = best;
                }
            }
        }
        input
    }

    /// `SignalGetter.getControlInputSignal`.
    fn control_input(&self, pos: [i32; 3], toward: Direction, diodes_only: bool) -> u8 {
        let Some(state) = self.world.signal_block(pos) else {
            return 0;
        };
        if diodes_only {
            return if is_diode(state) {
                self.direct(pos, toward)
            } else {
                0
            };
        }
        if name(state) == Some("redstone_block") {
            15
        } else if is_wire(state) {
            property_u8(state, "power").unwrap_or(0)
        } else if is_signal_source(state) {
            self.direct(pos, toward)
        } else {
            0
        }
    }

    /// `DiodeBlock.getAlternateSignal`: the larger of the two side inputs.
    fn side_input(&self, pos: [i32; 3], facing: Direction, diodes_only: bool) -> u8 {
        [clockwise(facing), counter_clockwise(facing)]
            .into_iter()
            .map(|side| self.control_input(offset(pos, side), side, diodes_only))
            .max()
            .unwrap_or(0)
    }

    /// `ComparatorBlock.calculateOutputSignal`.
    fn comparator_output(&self, pos: [i32; 3], state: u16) -> u8 {
        let Some(facing) = facing(state) else {
            return 0;
        };
        let input = self.comparator_input(pos, facing);
        let side = self.side_input(pos, facing, false);
        if side > input {
            0
        } else if property(state, "mode") == Some("subtract") {
            input - side
        } else {
            input
        }
    }
}

/// Power arriving at `pos` from `direction` (`Level.getSignal(pos.relative(direction), direction)`).
fn power_from(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    direction: Direction,
    analog: &HashMap<[i32; 3], u8>,
) -> u8 {
    Probe::new(world, analog).signal(offset(pos, direction), direction)
}

/// Every direction an adjacent block can power `pos` from (`Level.getBestNeighborSignal`).
fn input_power(world: &impl RedstoneWorld, pos: [i32; 3], analog: &HashMap<[i32; 3], u8>) -> u8 {
    Probe::new(world, analog).neighbor_signal(pos)
}

/// `RedStoneWireBlock.shouldConnectTo`.
fn should_connect_to(state: u16, direction: Option<Direction>) -> bool {
    match name(state) {
        Some("redstone_wire") => true,
        Some("repeater") => {
            facing(state).is_some_and(|f| Some(f) == direction || Some(f.opposite()) == direction)
        }
        Some("observer") => direction.is_some() && facing(state) == direction,
        _ => direction.is_some() && is_signal_source(state),
    }
}

/// `RedStoneWireBlock.canSurviveOn`: a sturdy top, or a hopper.
fn can_hold_wire(state: u16) -> bool {
    !ferrite_data::block_props::face_occlusion(state, face_index(Direction::Up)).is_empty()
        || name(state) == Some("hopper")
}

/// `RedStoneWireBlock.getConnectingSide`.
fn connecting_side(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    direction: Direction,
    non_solid_above: bool,
) -> &'static str {
    let neighbour = offset(pos, direction);
    let Some(state) = world.signal_block(neighbour) else {
        return "none";
    };
    if non_solid_above {
        let on_top =
            name(state).is_some_and(|name| name.ends_with("_trapdoor")) || can_hold_wire(state);
        let above_connects = world
            .signal_block(offset(neighbour, Direction::Up))
            .is_some_and(|above| should_connect_to(above, None));
        if on_top && above_connects {
            let sturdy = !ferrite_data::block_props::face_occlusion(
                state,
                face_index(direction.opposite()),
            )
            .is_empty();
            return if sturdy { "up" } else { "side" };
        }
    }
    let below_connects = world
        .signal_block(offset(neighbour, Direction::Down))
        .is_some_and(|below| should_connect_to(below, None));
    if !should_connect_to(state, Some(direction)) && (is_conductor(state) || !below_connects) {
        "none"
    } else {
        "side"
    }
}

/// `RedStoneWireBlock.getConnectionState`: sides in north, east, south, west order. A dot
/// with nothing to connect to stays a dot; any other wire without a line along an axis
/// reaches out to both sides of that axis.
fn wire_shape(world: &impl RedstoneWorld, pos: [i32; 3], old: u16) -> [&'static str; 4] {
    let non_solid_above = world
        .signal_block(offset(pos, Direction::Up))
        .is_none_or(|above| !is_conductor(above));
    let mut shape =
        HORIZONTAL.map(|direction| connecting_side(world, pos, direction, non_solid_above));
    let was_dot = HORIZONTAL
        .iter()
        .all(|&direction| property(old, connection_key(direction)) == Some("none"));
    if was_dot && shape.iter().all(|side| *side == "none") {
        return shape;
    }
    let [north, east, south, west] = shape.map(|side| side != "none");
    let no_north_south = !north && !south;
    let no_east_west = !east && !west;
    if !west && no_north_south {
        shape[3] = "side";
    }
    if !east && no_north_south {
        shape[1] = "side";
    }
    if !north && no_east_west {
        shape[0] = "side";
    }
    if !south && no_east_west {
        shape[2] = "side";
    }
    shape
}

fn with_wire_shape(
    state: u16,
    power: u8,
    world: &impl RedstoneWorld,
    pos: [i32; 3],
) -> Option<u16> {
    let shape = wire_shape(world, pos, state);
    let mut changes = Vec::with_capacity(5);
    changes.push(("power", POWER[power as usize]));
    for (direction, side) in HORIZONTAL.into_iter().zip(shape) {
        changes.push((connection_key(direction), side));
    }
    with_properties(state, &changes)
}

/// The wires a wire at `pos` exchanges power with (`RedStoneWireBlock.calculateTargetStrength`):
/// horizontal neighbours, plus the wire one step up over a conductor (unless the block above
/// this wire is one) or one step down off a non-conductor.
fn wire_links(world: &impl RedstoneWorld, pos: [i32; 3]) -> Vec<[i32; 3]> {
    let above_clear = world
        .signal_block(offset(pos, Direction::Up))
        .is_some_and(|above| !is_conductor(above));
    let mut links = Vec::with_capacity(8);
    for direction in HORIZONTAL {
        let neighbour = offset(pos, direction);
        let Some(state) = world.signal_block(neighbour) else {
            continue;
        };
        links.push(neighbour);
        if is_conductor(state) {
            if above_clear {
                links.push(offset(neighbour, Direction::Up));
            }
        } else {
            links.push(offset(neighbour, Direction::Down));
        }
    }
    links.retain(|&link| world.signal_block(link).is_some_and(is_wire));
    links
}

/// Connected wire network around `start`, bounded by [`WIRE_LIMIT`]. Only wires this region
/// owns are members; wires across the boundary feed it through [`wire_powers`].
fn wire_component(world: &impl RedstoneWorld, start: [i32; 3]) -> Vec<[i32; 3]> {
    let mut seen = HashSet::new();
    let mut wires = Vec::new();
    let mut queue = VecDeque::from([start]);
    while let Some(pos) = queue.pop_front() {
        if wires.len() >= WIRE_LIMIT {
            break;
        }
        if !seen.insert(pos) {
            continue;
        }
        if !world.block(pos).is_some_and(is_wire) {
            continue;
        }
        wires.push(pos);
        for link in wire_links(world, pos) {
            if !seen.contains(&link) {
                queue.push_back(link);
            }
        }
    }
    wires
}

/// Recompute a wire network from its non-wire sources, then relax power along the links.
/// This is `RedStoneWireBlock.calculateTargetStrength` applied until the network settles.
fn wire_powers(
    world: &impl RedstoneWorld,
    wires: &[[i32; 3]],
    analog: &HashMap<[i32; 3], u8>,
) -> HashMap<[i32; 3], u8> {
    let members: HashSet<[i32; 3]> = wires.iter().copied().collect();
    let mut power: HashMap<[i32; 3], u8> = wires.iter().map(|&pos| (pos, 0)).collect();
    let mut links: HashMap<[i32; 3], Vec<[i32; 3]>> = HashMap::new();
    let mut queue = VecDeque::new();
    let probe = Probe {
        world,
        analog,
        wires: false,
    };
    for &pos in wires {
        // `getBestNeighborSignal` with wires silenced, then the neighbours' own wires.
        let mut target = probe.neighbor_signal(pos);
        let around = wire_links(world, pos);
        for &link in &around {
            if !members.contains(&link)
                && let Some(state) = world.signal_block(link)
            {
                // A wire across the boundary feeds this network with vanilla attenuation.
                // Its own region keeps its power up to date and wakes this one on changes.
                target = target.max(property_u8(state, "power").unwrap_or(0).saturating_sub(1));
            }
        }
        links.insert(pos, around);
        if target > 0 {
            power.insert(pos, target);
            queue.push_back(pos);
        }
    }
    while let Some(pos) = queue.pop_front() {
        let strength = power[&pos];
        if strength <= 1 {
            continue;
        }
        for &neighbour in &links[&pos] {
            if members.contains(&neighbour) && power[&neighbour] < strength - 1 {
                power.insert(neighbour, strength - 1);
                queue.push_back(neighbour);
            }
        }
    }
    power
}

/// Toggle the blocks a player right-clicks (`LeverBlock.use`, `ButtonBlock.use`).
pub fn use_block(state: u16) -> Option<u16> {
    match name(state) {
        Some("lever") => toggle(state, "powered"),
        Some(name) if name.ends_with("_button") => with_properties(state, &[("powered", "true")]),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Diode,
    Torch,
    Button,
    ObserverPulse,
    ObserverOff,
    Daylight,
    LampOff,
}

/// Vanilla `TickPriority`: lower runs first among ticks due together.
const EXTREMELY_HIGH: i8 = -3;
const VERY_HIGH: i8 = -2;
const HIGH: i8 = -1;
const NORMAL: i8 = 0;

/// One region's redstone state: scheduled ticks, dirty blocks and the wire work list.
/// Something standing on a pressure plate (vanilla `BasePressurePlateBlock.getEntityCount`
/// counts entities whose box meets the plate's touch box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Occupant {
    /// Feet position.
    pub pos: [f64; 3],
    /// Half the box width, and its height.
    pub half: f64,
    pub height: f64,
    /// A `LivingEntity` (stone plates ignore everything else).
    pub living: bool,
}

impl Occupant {
    /// A player's box: 0.6 wide, 1.8 tall.
    pub fn player(pos: [f64; 3]) -> Self {
        Self {
            pos,
            half: 0.3,
            height: 1.8,
            living: true,
        }
    }

    /// A player's box in a pose: 1.8 tall standing, 1.5 sneaking, 0.2 square and tall asleep.
    pub fn player_in_pose(pos: [f64; 3], sleeping: bool, sneaking: bool) -> Self {
        let (half, height) = match (sleeping, sneaking) {
            (true, _) => (0.1, 0.2),
            (false, true) => (0.3, 1.5),
            _ => (0.3, 1.8),
        };
        Self {
            pos,
            half,
            height,
            living: true,
        }
    }

    /// `BasePressurePlateBlock.TOUCH_AABB` (14 px wide, 4 px tall) moved to `plate`.
    fn touches(&self, plate: [i32; 3]) -> bool {
        let (px, py, pz) = (
            f64::from(plate[0]),
            f64::from(plate[1]),
            f64::from(plate[2]),
        );
        self.pos[0] + self.half > px + 0.0625
            && self.pos[0] - self.half < px + 0.9375
            && self.pos[2] + self.half > pz + 0.0625
            && self.pos[2] - self.half < pz + 0.9375
            && self.pos[1] + self.height > py
            && self.pos[1] < py + 0.25
    }
}

#[derive(Debug, Default)]
pub struct Redstone {
    /// Entities other than players this tick (set by the region before [`Redstone::tick`]).
    occupants: Vec<Occupant>,
    /// When a pressed plate looks again (`getPressedTime` after the last check).
    plate_due: HashMap<[i32; 3], u64>,
    clock: u64,
    scheduled: HashMap<([i32; 3], Kind), u64>,
    /// `TickPriority` of a scheduled tick (absent means normal).
    priorities: HashMap<([i32; 3], Kind), i8>,
    /// Insertion order of a scheduled tick (`TickNextTickData.subTickOrder`): ticks due at the
    /// same time with the same priority run in the order they were scheduled.
    sequence: HashMap<([i32; 3], Kind), u64>,
    next_sequence: u64,
    dirty: VecDeque<[i32; 3]>,
    wires: HashSet<[i32; 3]>,
    /// Computed comparator strength per position (the state only carries `powered`).
    analog: HashMap<[i32; 3], u8>,
    /// Trapped chests with a non-zero viewer count in `analog`.
    chests: HashSet<[i32; 3]>,
    /// Recent off->on transitions for `RedstoneTorchBlock` burnout.
    toggles: HashMap<[i32; 3], VecDeque<u64>>,
    plates: HashSet<[i32; 3]>,
    daylights: HashSet<[i32; 3]>,
    primed: Vec<([i32; 3], i32)>,
    piston_candidates: HashSet<[i32; 3]>,
}

impl Redstone {
    fn schedule(&mut self, pos: [i32; 3], kind: Kind, delay: u64) {
        self.schedule_with(pos, kind, delay, NORMAL);
    }

    /// `Level.scheduleTick(pos, block, delay, priority)`: a tick already pending for the
    /// same block keeps its original time and priority, as in vanilla's tick queue.
    fn schedule_with(&mut self, pos: [i32; 3], kind: Kind, delay: u64, priority: i8) {
        let due = self.clock + delay;
        if let std::collections::hash_map::Entry::Vacant(slot) =
            self.scheduled.entry((pos, kind))
        {
            slot.insert(due);
            self.sequence.insert((pos, kind), self.next_sequence);
            self.next_sequence += 1;
            if priority != NORMAL {
                self.priorities.insert((pos, kind), priority);
            }
        }
    }

    /// `TickNextTickData`/`willTickThisTick`: a tick for this block is already due now.
    fn will_tick_this_tick(&self, pos: [i32; 3], kind: Kind) -> bool {
        self.scheduled
            .get(&(pos, kind))
            .is_some_and(|due| *due <= self.clock)
    }

    /// `Level.markAndNotifyBlock`: notify the changed block and its six neighbours.
    pub fn blocks_changed(&mut self, world: &mut impl RedstoneWorld, positions: &[[i32; 3]]) {
        for &pos in positions {
            self.after_change(world, pos, true);
        }
    }

    /// Re-evaluates a change another region made across the boundary. It never wakes back to
    /// the source, so two regions cannot ping-pong the same edge forever.
    pub fn foreign_changed(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], origin: [i32; 3]) {
        self.after_change(world, pos, false);
        // `ObserverBlock.updateShape`: an observer looking at the changed block across the
        // boundary pulses; the region that made the change cannot schedule it.
        if let Some(state) = world.block(pos)
            && is_observer(state)
            && facing(state).is_some_and(|facing| offset(pos, facing) == origin)
            && !property_bool(state, "powered")
        {
            self.schedule(pos, Kind::ObserverPulse, 2);
        }
    }

    pub fn take_primed(&mut self) -> Vec<([i32; 3], i32)> {
        std::mem::take(&mut self.primed)
    }

    pub fn take_piston_actions(
        &mut self,
        world: &impl RedstoneWorld,
    ) -> Vec<([i32; 3], Direction, bool)> {
        let candidates = std::mem::take(&mut self.piston_candidates);
        candidates
            .into_iter()
            .filter_map(|pos| {
                let state = world.block(pos)?;
                if !matches!(name(state), Some("piston" | "sticky_piston")) {
                    return None;
                }
                let facing = facing(state)?;
                let powered = super::piston::powered(pos, facing, |at, side| {
                    power_from(world, at, side, &self.analog) > 0
                });
                (powered != property_bool(state, "extended")).then_some((pos, facing, powered))
            })
            .collect()
    }

    /// The redstone power arriving at a block (`Level.getSignal`), including comparator
    /// outputs this region computed. Dispensers use it for their rising edge.
    pub fn power_at(&self, world: &impl RedstoneWorld, pos: [i32; 3]) -> u8 {
        input_power(world, pos, &self.analog)
    }

    pub fn tick(&mut self, world: &mut impl RedstoneWorld, players: &[[f64; 3]]) {
        self.clock = self.clock.wrapping_add(1);
        // `BasePressurePlateBlock.entityInside`: plates loaded from disk are found from the
        // player's feet block, not only when a neighbour changes.
        let standing: Vec<[f64; 3]> = players
            .iter()
            .copied()
            .chain(self.occupants.iter().map(|o| o.pos))
            .collect();
        for player in &standing {
            for dy in [0.0, -1.0] {
                let pos = [
                    player[0].floor() as i32,
                    (player[1] + dy).floor() as i32,
                    player[2].floor() as i32,
                ];
                if world.block(pos).is_some_and(is_pressure_plate) {
                    self.plates.insert(pos);
                }
            }
        }
        if self.scheduled.is_empty()
            && self.dirty.is_empty()
            && self.wires.is_empty()
            && self.plates.is_empty()
            && self.daylights.is_empty()
        {
            return;
        }
        self.run_scheduled(world);
        self.tick_plates(world, players);
        self.flush_dirty(world);
        self.update_wires(world);
        // Wire writes can change adjacent components in the same tick.
        self.flush_dirty(world);
        if self.clock.is_multiple_of(20) {
            self.tick_daylights(world);
        }
    }

    fn run_scheduled(&mut self, world: &mut impl RedstoneWorld) {
        // Vanilla runs ticks due together by (time, priority, insertion order).
        let mut due: Vec<(u64, i8, u64, [i32; 3], Kind)> = self
            .scheduled
            .iter()
            .filter(|&(_, &time)| time <= self.clock)
            .map(|(&(pos, kind), &time)| {
                let priority = self.priorities.get(&(pos, kind)).copied().unwrap_or(NORMAL);
                let order = self.sequence.get(&(pos, kind)).copied().unwrap_or(0);
                (time, priority, order, pos, kind)
            })
            .collect();
        due.sort_by_key(|&(time, priority, order, pos, _)| (time, priority, order, pos));
        due.truncate(UPDATE_LIMIT);
        for (_, _, _, pos, kind) in due {
            self.scheduled.remove(&(pos, kind));
            self.priorities.remove(&(pos, kind));
            self.sequence.remove(&(pos, kind));
            match kind {
                Kind::Diode => self.update_diode(world, pos),
                Kind::Torch => self.update_torch(world, pos),
                Kind::Button => self.release_button(world, pos),
                Kind::ObserverPulse => self.pulse_observer(world, pos),
                Kind::ObserverOff => self.unpulse_observer(world, pos),
                Kind::Daylight => self.update_daylight(world, pos),
                Kind::LampOff => self.lamp_off(world, pos),
            }
        }
    }

    fn flush_dirty(&mut self, world: &mut impl RedstoneWorld) {
        let mut budget = UPDATE_LIMIT;
        while let Some(pos) = self.dirty.pop_front() {
            if budget == 0 {
                self.dirty.clear();
                break;
            }
            budget -= 1;
            self.after_change(world, pos, true);
        }
    }

    fn update_wires(&mut self, world: &mut impl RedstoneWorld) {
        let mut budget = 256;
        while budget > 0 {
            let Some(pos) = self.wires.iter().next().copied() else {
                break;
            };
            self.wires.remove(&pos);
            budget -= 1;
            let wires = wire_component(world, pos);
            if wires.is_empty() {
                continue;
            }
            let powers = wire_powers(world, &wires, &self.analog);
            for wire in wires {
                let Some(state) = world.block(wire) else {
                    continue;
                };
                let power = powers.get(&wire).copied().unwrap_or(0);
                let Some(next) = with_wire_shape(state, power, world, wire) else {
                    continue;
                };
                if next != state {
                    self.write(world, wire, next);
                    // A wire that changed may reach components directly next to it.
                    for direction in Direction::ALL {
                        let neighbour = offset(wire, direction);
                        if let Some(state) = world.block(neighbour)
                            && !is_wire(state)
                        {
                            self.update_component(world, neighbour, state);
                        }
                    }
                }
            }
        }
    }

    fn write(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        if world.block(pos) == Some(state) {
            return;
        }
        world.set_block(pos, state);
        self.dirty.push_back(pos);
    }

    fn after_change(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], wake: bool) {
        let mut wakes = Vec::new();
        // A piston can be powered by a neighbour or by quasi-connectivity above it.
        for dx in -2..=2 {
            for dy in -2..=1 {
                for dz in -2..=2 {
                    let candidate = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                    let Some(candidate_state) = world.block(candidate) else {
                        continue;
                    };
                    if matches!(name(candidate_state), Some("piston" | "sticky_piston")) {
                        self.piston_candidates.insert(candidate);
                    }
                    // Wires link diagonally up and down staircases, so a change reaches wires
                    // one block away in any direction.
                    if is_wire(candidate_state)
                        && dx.abs() <= 1
                        && dy.abs() <= 1
                        && dz.abs() <= 1
                    {
                        self.wires.insert(candidate);
                    }
                }
            }
        }
        if world.block(pos).is_some_and(is_wire) {
            self.wires.insert(pos);
        } else if let Some(state) = world.block(pos) {
            self.update_component(world, pos, state);
        }
        for direction in UPDATE_ORDER {
            let neighbour = offset(pos, direction);
            let Some(state) = world.block(neighbour) else {
                if wake {
                    wakes.push((neighbour, pos));
                }
                continue;
            };
            if is_wire(state) {
                self.wires.insert(neighbour);
            } else if is_observer(state) && facing(state) == Some(direction.opposite()) {
                // `ObserverBlock.updateShape`: the observer saw a block change in front;
                // one that is already pulsing ignores it.
                if !property_bool(state, "powered") {
                    self.schedule(neighbour, Kind::ObserverPulse, 2);
                }
            } else {
                self.update_component(world, neighbour, state);
            }
        }
        // Strong power reaches components through one conductive block. A lever on a wall
        // powers the stone behind it, which in turn powers the torch standing on that stone.
        for direction in UPDATE_ORDER {
            let carrier = offset(pos, direction);
            let Some(carrier_state) = world.block(carrier) else {
                if wake {
                    wakes.push((carrier, pos));
                }
                continue;
            };
            if !is_conductor(carrier_state) {
                continue;
            }
            for inner in UPDATE_ORDER {
                let target = offset(carrier, inner);
                if target == pos {
                    continue;
                }
                if let Some(state) = world.block(target) {
                    if is_wire(state) {
                        self.wires.insert(target);
                    } else {
                        // Lamps, note blocks and the like are conductive consumers too.
                        self.update_component(world, target, state);
                    }
                } else if wake {
                    // Strong power carried through this conductor reaches a block the
                    // neighbouring region owns.
                    wakes.push((target, pos));
                }
            }
        }
        wakes.sort_unstable();
        wakes.dedup();
        if !wakes.is_empty() {
            world.wake_redstone(&wakes);
        }
    }

    fn update_component(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let Some(name) = name(state) else {
            return;
        };
        match name {
            "redstone_wire" => {
                self.wires.insert(pos);
            }
            "repeater" => {
                // `RepeaterBlock.updateShape` refreshes `locked`; `DiodeBlock.neighborChanged`
                // then runs `checkTickOnNeighbor`.
                self.update_lock(world, pos, state);
                let state = world.block(pos).unwrap_or(state);
                self.check_repeater_tick(world, pos, state);
            }
            "comparator" => self.check_comparator_tick(world, pos, state),
            "redstone_torch" | "redstone_wall_torch" => {
                // `RedstoneTorchBlock.neighborChanged`: a torch that disagrees with its input
                // looks again in two ticks, unless it is already due.
                let input = torch_input(world, pos, state, &self.analog);
                if property_bool(state, "lit") == input && !self.will_tick_this_tick(pos, Kind::Torch)
                {
                    self.schedule(pos, Kind::Torch, 2);
                }
            }
            "redstone_lamp" => {
                // `RedstoneLampBlock.neighborChanged`: lights at once, goes dark after 4 ticks.
                let signal = input_power(world, pos, &self.analog) > 0;
                let lit = property_bool(state, "lit");
                if lit && !signal {
                    self.schedule(pos, Kind::LampOff, 4);
                } else if !lit
                    && signal
                    && let Some(next) = with_properties(state, &[("lit", "true")])
                {
                    self.write(world, pos, next);
                }
            }
            "note_block" => {
                // `NoteBlock.neighborChanged` only follows the signal; playing the note
                // does not change it.
                let powered = input_power(world, pos, &self.analog) > 0;
                if powered != property_bool(state, "powered")
                    && let Some(next) = with_properties(
                        state,
                        &[("powered", if powered { "true" } else { "false" })],
                    )
                {
                    self.write(world, pos, next);
                }
            }
            "tnt" => {
                if input_power(world, pos, &self.analog) > 0 {
                    let air = blocks::default_state("air").unwrap_or(0);
                    self.write(world, pos, air);
                    self.primed.push((pos, ferrite_entity::tnt::FUSE));
                }
            }
            "powered_rail" => {
                let powered = rail_powered(world, pos, state, &self.analog);
                if powered != property_bool(state, "powered")
                    && let Some(next) = with_properties(
                        state,
                        &[("powered", if powered { "true" } else { "false" })],
                    )
                {
                    self.write(world, pos, next);
                    // `updateNeighborsAt(pos.below())`, and above on a slope, reach the rails
                    // diagonally up and down the track.
                    self.dirty.push_back(offset(pos, Direction::Down));
                    if property(state, "shape").is_some_and(|shape| shape.starts_with("ascending")) {
                        self.dirty.push_back(offset(pos, Direction::Up));
                    }
                }
            }
            "daylight_detector" => {
                self.daylights.insert(pos);
                self.schedule(pos, Kind::Daylight, 20);
                self.update_daylight(world, pos);
            }
            _ if name.ends_with("_button") => {
                self.plates.remove(&pos);
                if property_bool(state, "powered") {
                    let delay = if name.starts_with("stone_") {
                        STONE_BUTTON_TICKS
                    } else {
                        WOOD_BUTTON_TICKS
                    };
                    self.schedule(pos, Kind::Button, delay);
                }
            }
            _ if name.ends_with("_pressure_plate") => {
                self.plates.insert(pos);
            }
            _ if name.ends_with("_door") => self.update_door(world, pos, state),
            _ if name.ends_with("_trapdoor") || name.ends_with("_fence_gate") => {
                self.update_openable(world, pos, state);
            }
            _ => {}
        }
    }

    fn update_openable(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let powered = input_power(world, pos, &self.analog) > 0;
        if powered == property_bool(state, "powered") {
            return;
        }
        let open = if powered { "true" } else { "false" };
        if let Some(next) = with_properties(state, &[("powered", open), ("open", open)]) {
            self.write(world, pos, next);
        }
    }
    fn update_door(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        // Vanilla `DoorBlock.getInputSignal`: the upper half reads the signal at the lower
        // half, so the two halves never fight over the power state.
        let (pos, state) = if property(state, "half") == Some("upper") {
            let lower = offset(pos, Direction::Down);
            match world.block(lower) {
                Some(lower_state) if name(lower_state) == name(state) => (lower, lower_state),
                _ => return,
            }
        } else {
            (pos, state)
        };
        // `DoorBlock.neighborChanged`: either half's neighbours power the door.
        let powered = input_power(world, pos, &self.analog) > 0
            || input_power(world, offset(pos, Direction::Up), &self.analog) > 0;
        if powered == property_bool(state, "powered") {
            return;
        }
        let open = if powered { "true" } else { "false" };
        if let Some(next) = with_properties(state, &[("powered", open), ("open", open)]) {
            self.write(world, pos, next);
        }
        // `DoorBlock.setOpen` updates the other half too.
        let other_pos = offset(pos, Direction::Up);
        if let Some(other_state) = world.block(other_pos)
            && name(other_state) == name(state)
            && let Some(next) = with_properties(other_state, &[("powered", open), ("open", open)])
        {
            self.write(world, other_pos, next);
        }
    }

    fn release_button(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if is_button(state)
            && property_bool(state, "powered")
            && let Some(next) = with_properties(state, &[("powered", "false")])
        {
            self.write(world, pos, next);
        }
    }

    /// `RepeaterBlock.updateShape`: the `locked` property follows a powered diode on a side.
    fn update_lock(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let Some(direction) = facing(state) else {
            return;
        };
        // Comparators have no `locked` property and are never locked.
        if property(state, "locked").is_none() {
            return;
        }
        let locked = is_locked(world, pos, direction, &self.analog);
        if locked != property_bool(state, "locked")
            && let Some(next) =
                with_properties(state, &[("locked", if locked { "true" } else { "false" })])
        {
            self.write(world, pos, next);
        }
    }

    /// `DiodeBlock.checkTickOnNeighbor` for a repeater.
    fn check_repeater_tick(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let Some(facing) = facing(state) else {
            return;
        };
        if is_locked(world, pos, facing, &self.analog) {
            return;
        }
        let powered = property_bool(state, "powered");
        let should_turn_on = Probe::new(world, &self.analog).diode_input(pos, facing) > 0;
        if powered == should_turn_on || self.will_tick_this_tick(pos, Kind::Diode) {
            return;
        }
        // `shouldPrioritize`: a diode in front that points across this one reads it first.
        let front = offset(pos, facing.opposite());
        let priority = if world
            .signal_block(front)
            .is_some_and(|other| is_diode(other) && self::facing(other) != Some(facing.opposite()))
        {
            EXTREMELY_HIGH
        } else if powered {
            VERY_HIGH
        } else {
            HIGH
        };
        let delay = u64::from(property_u8(state, "delay").unwrap_or(1)) * 2;
        self.schedule_with(pos, Kind::Diode, delay, priority);
    }

    /// `ComparatorBlock.checkTickOnNeighbor`: look again when the output or `powered` is stale.
    fn check_comparator_tick(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let Some(facing) = facing(state) else {
            return;
        };
        if self.will_tick_this_tick(pos, Kind::Diode) {
            return;
        }
        let powered = property_bool(state, "powered");
        let output = Probe::new(world, &self.analog).comparator_output(pos, state);
        // The output lives in a block entity in vanilla; a powered comparator whose output
        // this region has not computed yet counts as stale.
        let stored = self
            .analog
            .get(&pos)
            .copied()
            .unwrap_or(if powered { u8::MAX } else { 0 });
        if output == stored && powered == (output > 0) {
            return;
        }
        let front = offset(pos, facing.opposite());
        let priority = if world
            .signal_block(front)
            .is_some_and(|other| is_diode(other) && self::facing(other) != Some(facing.opposite()))
        {
            HIGH
        } else {
            NORMAL
        };
        self.schedule_with(pos, Kind::Diode, 2, priority);
    }

    /// The tick of a repeater (`DiodeBlock.tick`) or comparator (`ComparatorBlock.tick`).
    fn update_diode(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        let Some(facing) = facing(state) else {
            return;
        };
        if is_repeater(state) {
            if is_locked(world, pos, facing, &self.analog) {
                return;
            }
            let powered = property_bool(state, "powered");
            let should_turn_on = Probe::new(world, &self.analog).diode_input(pos, facing) > 0;
            if powered && !should_turn_on {
                if let Some(next) = with_properties(state, &[("powered", "false")]) {
                    self.write(world, pos, next);
                }
            } else if !powered {
                if let Some(next) = with_properties(state, &[("powered", "true")]) {
                    self.write(world, pos, next);
                }
                if !should_turn_on {
                    // A pulse shorter than the delay is stretched to the full delay.
                    let delay = u64::from(property_u8(state, "delay").unwrap_or(1)) * 2;
                    self.schedule_with(pos, Kind::Diode, delay, VERY_HIGH);
                }
            }
            return;
        }
        // `ComparatorBlock.refreshOutputState`.
        let output = Probe::new(world, &self.analog).comparator_output(pos, state);
        let previous = self.analog.insert(pos, output);
        if previous != Some(output) {
            // The comparator's state only carries `powered`; a change between two non-zero
            // strengths must still refresh what it feeds.
            for direction in Direction::ALL {
                let neighbour = offset(pos, direction);
                if world.block(neighbour).is_some_and(is_wire) {
                    self.wires.insert(neighbour);
                } else if let Some(state) = world.block(neighbour) {
                    self.update_component(world, neighbour, state);
                }
            }
        }
        let powered = output > 0;
        if powered != property_bool(state, "powered")
            && let Some(next) = with_properties(
                state,
                &[("powered", if powered { "true" } else { "false" })],
            )
        {
            self.write(world, pos, next);
        }
    }

    /// `RedstoneLampBlock.tick`: a lit lamp without a signal goes dark.
    fn lamp_off(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if name(state) == Some("redstone_lamp")
            && property_bool(state, "lit")
            && input_power(world, pos, &self.analog) == 0
            && let Some(next) = with_properties(state, &[("lit", "false")])
        {
            self.write(world, pos, next);
        }
    }

    fn update_torch(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if !is_torch(state) {
            return;
        }
        let input = torch_input(world, pos, state, &self.analog);
        let lit = property_bool(state, "lit");
        // `RedstoneTorchBlock.tick`: forget toggles older than 60 ticks (`RECENT_TOGGLES`).
        let clock = self.clock;
        let recent = self.toggles.entry(pos).or_default();
        while recent
            .front()
            .is_some_and(|&time| clock.saturating_sub(time) > TORCH_TOGGLE_WINDOW)
        {
            recent.pop_front();
        }
        if lit {
            if !input {
                return;
            }
            // Turning off is what counts; the eighth toggle in the window burns the torch out
            // and it looks again after 160 ticks (`isToggledTooFrequently(level, pos, true)`).
            recent.push_back(clock);
            let burnt_out = recent.len() >= TORCH_MAX_TOGGLES;
            if let Some(next) = with_properties(state, &[("lit", "false")]) {
                self.write(world, pos, next);
            }
            if burnt_out {
                self.schedule(pos, Kind::Torch, TORCH_BURNOUT_TICKS);
            }
        } else if !input
            && recent.len() < TORCH_MAX_TOGGLES
            && let Some(next) = with_properties(state, &[("lit", "true")])
        {
            self.write(world, pos, next);
        }
    }

    fn pulse_observer(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if !is_observer(state) || property_bool(state, "powered") {
            return;
        }
        if let Some(next) = with_properties(state, &[("powered", "true")]) {
            self.write(world, pos, next);
            self.schedule(pos, Kind::ObserverOff, 2);
        }
    }

    fn unpulse_observer(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if is_observer(state)
            && property_bool(state, "powered")
            && let Some(next) = with_properties(state, &[("powered", "false")])
        {
            self.write(world, pos, next);
        }
    }

    fn update_daylight(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if name(state) != Some("daylight_detector") {
            return;
        }
        let light = world.brightness(pos).min(15);
        let power = if property_bool(state, "inverted") {
            15 - light
        } else {
            light
        };
        if property_u8(state, "power") == Some(power) {
            return;
        }
        self.daylights.insert(pos);
        self.schedule(pos, Kind::Daylight, 20);
        if let Some(next) = with_properties(state, &[("power", POWER[usize::from(power)])]) {
            self.write(world, pos, next);
        }
    }

    fn tick_daylights(&mut self, world: &mut impl RedstoneWorld) {
        let detectors: Vec<_> = self.daylights.iter().copied().collect();
        for pos in detectors {
            self.update_daylight(world, pos);
        }
    }

    /// Vanilla `BasePressurePlateBlock`: a released plate presses as soon as something touches it
    /// (`entityInside`); a pressed one looks again only when its scheduled tick comes up, 20 ticks
    /// after the last look (10 for weighted plates), and keeps looking while it is occupied.
    fn tick_plates(&mut self, world: &mut impl RedstoneWorld, players: &[[f64; 3]]) {
        let occupants: Vec<Occupant> = players
            .iter()
            .map(|&pos| Occupant::player(pos))
            .chain(self.occupants.iter().copied())
            .collect();
        let plates: Vec<_> = self.plates.iter().copied().collect();
        for pos in plates {
            let Some(state) = world.block(pos) else {
                continue;
            };
            if !is_pressure_plate(state) {
                self.plates.remove(&pos);
                self.plate_due.remove(&pos);
                continue;
            }
            let Some(plate) = name(state) else {
                continue;
            };
            let weighted_max = match plate {
                "light_weighted_pressure_plate" => Some(15),
                "heavy_weighted_pressure_plate" => Some(150),
                _ => None,
            };
            let current = if weighted_max.is_some() {
                property_u8(state, "power").unwrap_or(0)
            } else if property_bool(state, "powered") {
                15
            } else {
                0
            };
            let looks = current == 0
                || self
                    .plate_due
                    .get(&pos)
                    .is_none_or(|due| *due <= self.clock);
            if !looks {
                continue;
            }
            // Stone and blackstone plates sense living entities only.
            let living_only =
                plate.starts_with("stone_") || plate.starts_with("polished_blackstone_");
            let count = occupants
                .iter()
                .filter(|o| (!living_only || o.living) && o.touches(pos))
                .count();
            let signal = match weighted_max {
                Some(max) if count > 0 => {
                    let held = count.min(max) as f32;
                    (held / max as f32 * 15.0).ceil() as u8
                }
                Some(_) => 0,
                None => u8::from(count > 0) * 15,
            };
            if signal != current {
                let changes: [(&str, String); 1] = if weighted_max.is_some() {
                    [("power", signal.to_string())]
                } else {
                    [("powered", (signal > 0).to_string())]
                };
                let refs = [(changes[0].0, changes[0].1.as_str())];
                if let Some(next) = with_properties(state, &refs) {
                    self.write(world, pos, next);
                }
            }
            if signal > 0 {
                let time = if weighted_max.is_some() { 10 } else { 20 };
                self.plate_due.insert(pos, self.clock + time);
            } else {
                self.plate_due.remove(&pos);
            }
        }
    }

    /// Replaces the item-frame strengths (`ItemFrame.getAnalogOutput`) the region computed:
    /// `(block, facing, strength)` per frame. A face with two frames counts as none, like
    /// `ComparatorBlock.getItemFrame`, which needs exactly one. Blocks whose frame output
    /// changed update their neighbours (`updateNeighbourForOutputSignal`).
    pub fn set_item_frames(&mut self, frames: &[([i32; 3], Direction, u8)]) {
        let mut counted: HashMap<[i32; 3], (u32, u8)> = HashMap::new();
        for &(pos, facing, strength) in frames {
            let entry = counted.entry(frame_key(pos, facing)).or_insert((0, strength));
            entry.0 += 1;
        }
        let next: HashMap<[i32; 3], u8> = counted
            .into_iter()
            .filter_map(|(key, (count, strength))| (count == 1).then_some((key, strength)))
            .collect();
        self.replace_outputs(
            &next,
            |key| key[1].abs() >= FRAME_BIAS / 2,
            |key| {
                let index = (key[1] + FRAME_BIAS / 2).div_euclid(FRAME_BIAS);
                [key[0], key[1] - index * FRAME_BIAS, key[2]]
            },
        );
    }

    /// Replaces the open-viewer counts of trapped chests (`TrappedChestBlock.getSignal`,
    /// clamped to 0..=15 when read). A chest nobody has open is simply absent.
    pub fn set_chest_viewers(&mut self, viewers: &HashMap<[i32; 3], u8>) {
        let next: HashMap<[i32; 3], u8> = viewers
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(&pos, &count)| (pos, count))
            .collect();
        let chests = std::mem::take(&mut self.chests);
        for pos in chests.iter().filter(|pos| !next.contains_key(*pos)).copied() {
            self.analog.remove(&pos);
            self.dirty.push_back(pos);
        }
        for (&pos, &count) in &next {
            if self.analog.insert(pos, count) != Some(count) {
                self.dirty.push_back(pos);
            }
        }
        self.chests = next.keys().copied().collect();
    }

    fn replace_outputs(
        &mut self,
        next: &HashMap<[i32; 3], u8>,
        is_member: impl Fn([i32; 3]) -> bool,
        block_of: impl Fn([i32; 3]) -> [i32; 3],
    ) {
        let stale: Vec<[i32; 3]> = self
            .analog
            .keys()
            .copied()
            .filter(|&key| is_member(key) && !next.contains_key(&key))
            .collect();
        for key in stale {
            self.analog.remove(&key);
            self.dirty.push_back(block_of(key));
        }
        for (&key, &strength) in next {
            if self.analog.insert(key, strength) != Some(strength) {
                self.dirty.push_back(block_of(key));
            }
        }
    }

    /// Entities other than players for this tick's pressure plates.
    pub fn set_occupants(&mut self, occupants: Vec<Occupant>) {
        self.occupants = occupants;
    }

    /// Whether item-frame outputs are currently held, so the region can clear them.
    pub fn has_frames(&self) -> bool {
        self.analog.keys().any(|key| key[1].abs() >= FRAME_BIAS / 2)
    }

    /// Whether any trapped chest has viewers recorded.
    pub fn has_chests(&self) -> bool {
        !self.chests.is_empty()
    }

    /// Whether any pressure plate is being watched, so the region can skip gathering entities.
    pub fn has_plates(&self) -> bool {
        !self.plates.is_empty()
    }

    /// Keep tick data with the half of the region that still owns it (`RegionData::split`).
    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self {
            clock: self.clock,
            ..Self::default()
        };
        let priorities = &mut self.priorities;
        let sequence = &mut self.sequence;
        out.next_sequence = self.next_sequence;
        self.scheduled.retain(|(pos, kind), due| {
            if keep(pos[0], pos[2]) {
                out.scheduled.insert((*pos, *kind), *due);
                if let Some(priority) = priorities.remove(&(*pos, *kind)) {
                    out.priorities.insert((*pos, *kind), priority);
                }
                if let Some(order) = sequence.remove(&(*pos, *kind)) {
                    out.sequence.insert((*pos, *kind), order);
                }
                false
            } else {
                true
            }
        });
        self.wires.retain(|pos| {
            if keep(pos[0], pos[2]) {
                out.wires.insert(*pos);
                false
            } else {
                true
            }
        });
        self.plate_due.retain(|pos, due| {
            if keep(pos[0], pos[2]) {
                out.plate_due.insert(*pos, *due);
                false
            } else {
                true
            }
        });
        self.plates.retain(|pos| {
            if keep(pos[0], pos[2]) {
                out.plates.insert(*pos);
                false
            } else {
                true
            }
        });
        self.daylights.retain(|pos| {
            if keep(pos[0], pos[2]) {
                out.daylights.insert(*pos);
                false
            } else {
                true
            }
        });
        self.analog.retain(|pos, power| {
            if keep(pos[0], pos[2]) {
                out.analog.insert(*pos, *power);
                false
            } else {
                true
            }
        });
        self.chests.retain(|pos| {
            if keep(pos[0], pos[2]) {
                out.chests.insert(*pos);
                false
            } else {
                true
            }
        });
        self.toggles.retain(|pos, recent| {
            if keep(pos[0], pos[2]) {
                out.toggles.insert(*pos, std::mem::take(recent));
                false
            } else {
                true
            }
        });
        self.dirty.retain(|pos| {
            if keep(pos[0], pos[2]) {
                out.dirty.push_back(*pos);
                false
            } else {
                true
            }
        });
        out
    }

    /// Merges another region's redstone work. Scheduled ticks and torch-toggle timestamps are
    /// absolute in the source region's clock, so both are re-based on this region's clock.
    pub fn merge(&mut self, other: Self) {
        self.priorities.extend(other.priorities);
        // Merged ticks queue behind the ones already here, keeping their own relative order.
        let mut incoming: Vec<_> = other.scheduled.into_iter().collect();
        incoming.sort_by_key(|(key, _)| other.sequence.get(key).copied().unwrap_or(0));
        for (key, due) in incoming {
            if !self.scheduled.contains_key(&key) {
                self.sequence.insert(key, self.next_sequence);
                self.next_sequence += 1;
            }
            let remaining = due.saturating_sub(other.clock);
            let due = self.clock.saturating_add(remaining);
            self.scheduled
                .entry(key)
                .and_modify(|current| *current = (*current).min(due))
                .or_insert(due);
        }
        self.dirty.extend(other.dirty);
        self.wires.extend(other.wires);
        self.analog.extend(other.analog);
        self.chests.extend(other.chests);
        self.plates.extend(other.plates);
        self.plate_due.extend(other.plate_due);
        self.daylights.extend(other.daylights);
        self.primed.extend(other.primed);
        for (pos, mut toggles) in other.toggles {
            for time in &mut toggles {
                *time = rebase(*time, other.clock, self.clock);
            }
            self.toggles.entry(pos).or_default().append(&mut toggles);
        }
    }
}

/// Moves an absolute clock value from one region timeline to another, never below zero.
fn rebase(time: u64, from: u64, to: u64) -> u64 {
    if to >= from {
        time.saturating_add(to - from)
    } else {
        time.saturating_sub(from - to)
    }
}

/// `RepeaterBlock.isLocked`: a powered diode on either side points into the repeater at `pos`.
fn is_locked(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    facing: Direction,
    analog: &HashMap<[i32; 3], u8>,
) -> bool {
    Probe::new(world, analog).side_input(pos, facing, true) > 0
}

/// `PoweredRailBlock.updateState`: a signal at the rail or, along the track, at a powered rail
/// of the same axis up to eight rails away in either direction.
fn rail_powered(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    state: u16,
    analog: &HashMap<[i32; 3], u8>,
) -> bool {
    input_power(world, pos, analog) > 0
        || rail_chain(world, pos, state, true, 0, analog)
        || rail_chain(world, pos, state, false, 0, analog)
}

/// `PoweredRailBlock.findPoweredRailSignal`.
fn rail_chain(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    state: u16,
    forward: bool,
    depth: i32,
    analog: &HashMap<[i32; 3], u8>,
) -> bool {
    if depth >= 8 {
        return false;
    }
    let [mut x, mut y, z] = pos;
    let mut z = z;
    let mut check = true;
    let axis_east_west;
    match property(state, "shape") {
        Some("north_south") => {
            z += if forward { 1 } else { -1 };
            axis_east_west = false;
        }
        Some("east_west") => {
            x += if forward { -1 } else { 1 };
            axis_east_west = true;
        }
        Some("ascending_east") => {
            if forward {
                x -= 1;
            } else {
                x += 1;
                y += 1;
                check = false;
            }
            axis_east_west = true;
        }
        Some("ascending_west") => {
            if forward {
                x -= 1;
                y += 1;
                check = false;
            } else {
                x += 1;
            }
            axis_east_west = true;
        }
        Some("ascending_north") => {
            if forward {
                z += 1;
            } else {
                z -= 1;
                y += 1;
                check = false;
            }
            axis_east_west = false;
        }
        Some("ascending_south") => {
            if forward {
                z += 1;
                y += 1;
                check = false;
            } else {
                z -= 1;
            }
            axis_east_west = false;
        }
        _ => return false,
    }
    rail_same_with_power(world, [x, y, z], forward, depth, axis_east_west, analog)
        || (check && rail_same_with_power(world, [x, y - 1, z], forward, depth, axis_east_west, analog))
}

/// `PoweredRailBlock.isSameRailWithPower`.
fn rail_same_with_power(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    forward: bool,
    depth: i32,
    axis_east_west: bool,
    analog: &HashMap<[i32; 3], u8>,
) -> bool {
    let Some(state) = world.signal_block(pos) else {
        return false;
    };
    if name(state) != Some("powered_rail") {
        return false;
    }
    let crosses = match property(state, "shape") {
        Some("north_south" | "ascending_north" | "ascending_south") => axis_east_west,
        Some("east_west" | "ascending_east" | "ascending_west") => !axis_east_west,
        _ => true,
    };
    if crosses || !property_bool(state, "powered") {
        return false;
    }
    input_power(world, pos, analog) > 0 || rail_chain(world, pos, state, forward, depth + 1, analog)
}

/// `RedstoneTorchBlock.hasNeighborSignal`: the block the torch hangs on is powered from any
/// side but the torch's own.
fn torch_input(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    state: u16,
    analog: &HashMap<[i32; 3], u8>,
) -> bool {
    let attach = match name(state) {
        Some("redstone_wall_torch") => facing(state)
            .map(Direction::opposite)
            .unwrap_or(Direction::Down),
        _ => Direction::Down,
    };
    Probe::new(world, analog).signal(offset(pos, attach), attach) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Grid {
        blocks: HashMap<[i32; 3], u16>,
        brightness: u8,
    }

    impl Grid {
        fn new() -> Self {
            Self {
                blocks: HashMap::new(),
                brightness: 15,
            }
        }

        fn add(&mut self, pos: [i32; 3], state: u16) {
            self.blocks.insert(pos, state);
        }

        fn state(&self, pos: [i32; 3]) -> u16 {
            self.block(pos).unwrap()
        }
    }

    impl RedstoneWorld for Grid {
        fn block(&self, pos: [i32; 3]) -> Option<u16> {
            Some(self.blocks.get(&pos).copied().unwrap_or_else(air))
        }

        fn set_block(&mut self, pos: [i32; 3], state: u16) {
            self.blocks.insert(pos, state);
        }

        fn brightness(&self, _pos: [i32; 3]) -> u8 {
            self.brightness
        }
    }

    fn air() -> u16 {
        blocks::default_state("air").expect("vanilla air")
    }

    #[test]
    fn piston_power_checks_quasi_connectivity_and_ignores_its_front() {
        let mut grid = Grid::new();
        let pos = [0, 64, 0];
        grid.add([1, 64, 0], make("redstone_block", &[]));
        let redstone = Redstone::default();
        let powered = |grid: &Grid| {
            super::super::piston::powered(pos, Direction::East, |at, side| {
                power_from(grid, at, side, &redstone.analog) > 0
            })
        };
        assert!(!powered(&grid));
        grid.add([1, 64, 0], air());
        grid.add([1, 65, 0], make("redstone_block", &[]));
        assert!(powered(&grid));
    }

    #[test]
    fn changed_power_source_schedules_a_nearby_piston() {
        let mut grid = Grid::new();
        let pos = [0, 64, 0];
        grid.add(
            pos,
            make("piston", &[("facing", "east"), ("extended", "false")]),
        );
        grid.add([-1, 64, 0], make("redstone_block", &[]));
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[-1, 64, 0]]);
        redstone.tick(&mut grid, &[]);
        assert!(
            redstone
                .take_piston_actions(&grid)
                .contains(&(pos, Direction::East, true))
        );
    }

    /// A region owning one side of the x=15/16 boundary. `signal_block` sees the other side.
    struct Edge {
        owns_left: bool,
        blocks: HashMap<[i32; 3], u16>,
        wakes: Vec<Wake>,
    }

    impl Edge {
        fn new(owns_left: bool) -> Self {
            Self {
                owns_left,
                blocks: HashMap::new(),
                wakes: Vec::new(),
            }
        }

        fn state(&self, pos: [i32; 3]) -> u16 {
            self.blocks.get(&pos).copied().unwrap_or_else(air)
        }
    }

    impl RedstoneWorld for Edge {
        fn block(&self, pos: [i32; 3]) -> Option<u16> {
            ((pos[0] <= 15) == self.owns_left).then(|| self.state(pos))
        }

        fn set_block(&mut self, pos: [i32; 3], state: u16) {
            self.blocks.insert(pos, state);
        }

        fn brightness(&self, _pos: [i32; 3]) -> u8 {
            15
        }

        fn signal_block(&self, pos: [i32; 3]) -> Option<u16> {
            Some(self.state(pos))
        }

        fn wake_redstone(&mut self, wakes: &[Wake]) {
            self.wakes.extend_from_slice(wakes);
        }
    }

    #[test]
    fn an_edge_change_wakes_the_neighbouring_region_once() {
        let mut world = Edge::new(true);
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut world, &[[15, 64, 0]]);
        assert_eq!(world.wakes, [([16, 64, 0], [15, 64, 0])]);
        world.wakes.clear();
        // Handling a foreign change must not bounce the wake back to its source.
        redstone.foreign_changed(&mut world, [15, 64, 0], [16, 64, 0]);
        assert!(world.wakes.is_empty());
    }

    #[test]
    fn a_lever_across_the_boundary_lights_a_lamp_on_this_side() {
        let mut world = Edge::new(false);
        world.blocks.insert(
            [15, 64, 0],
            make(
                "lever",
                &[("face", "floor"), ("facing", "east"), ("powered", "true")],
            ),
        );
        world.blocks.insert([16, 64, 0], lamp());
        let mut redstone = Redstone::default();
        redstone.foreign_changed(&mut world, [16, 64, 0], [15, 64, 0]);
        assert_eq!(
            world.state([16, 64, 0]),
            make("redstone_lamp", &[("lit", "true")])
        );
    }

    fn make(name: &str, changes: &[(&str, &str)]) -> u16 {
        let full = format!("minecraft:{name}");
        let base = blocks::default_state(&full).unwrap_or_else(|| panic!("vanilla {name}"));
        with_properties(base, changes).unwrap_or_else(|| panic!("properties for {name}"))
    }

    fn tick(redstone: &mut Redstone, grid: &mut Grid, ticks: usize) {
        for _ in 0..ticks {
            redstone.tick(grid, &[]);
        }
    }

    fn lever() -> u16 {
        make(
            "lever",
            &[("face", "floor"), ("facing", "north"), ("powered", "false")],
        )
    }

    fn wire() -> u16 {
        make("redstone_wire", &[("power", "0")])
    }

    fn lamp() -> u16 {
        make("redstone_lamp", &[("lit", "false")])
    }

    #[test]
    fn lever_powers_wire_and_lamp_and_turning_it_off_clears_the_wire() {
        let mut grid = Grid::new();
        let source = lever();
        grid.add([0, 64, 0], source);
        grid.add([1, 64, 0], wire());
        grid.add([2, 64, 0], lamp());
        let mut redstone = Redstone::default();

        let powered = use_block(source).unwrap();
        grid.add([0, 64, 0], powered);
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert_eq!(property(grid.state([1, 64, 0]), "power"), Some("15"));
        assert!(property_bool(grid.state([2, 64, 0]), "lit"));

        let off = use_block(powered).unwrap();
        grid.add([0, 64, 0], off);
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([1, 64, 0]), "power"), Some("0"));
        assert!(
            property_bool(grid.state([2, 64, 0]), "lit"),
            "`RedstoneLampBlock` waits 4 ticks before going dark"
        );
        tick(&mut redstone, &mut grid, 4);
        assert!(!property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn repeater_waits_delay_times_two_ticks_before_switching() {
        let mut grid = Grid::new();
        let source = lever();
        grid.add([0, 64, 0], source);
        grid.add(
            [1, 64, 0],
            make(
                "repeater",
                &[("facing", "west"), ("delay", "2"), ("powered", "false")],
            ),
        );
        grid.add([2, 64, 0], lamp());
        let mut redstone = Redstone::default();

        grid.add([0, 64, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 3);
        assert!(!property_bool(grid.state([2, 64, 0]), "lit"));
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn torch_inverts_the_block_it_is_attached_to() {
        let mut grid = Grid::new();
        grid.add([1, 63, 0], make("stone", &[]));
        grid.add([1, 64, 0], make("redstone_torch", &[("lit", "true")]));
        grid.add([2, 64, 0], lamp());
        let source = make(
            "lever",
            &[("face", "wall"), ("facing", "west"), ("powered", "false")],
        );
        grid.add([0, 63, 0], source);
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([2, 64, 0]), "lit"));

        grid.add([0, 63, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 63, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert!(!property_bool(grid.state([1, 64, 0]), "lit"));
        assert!(
            property_bool(grid.state([2, 64, 0]), "lit"),
            "the lamp keeps shining for its 4-tick delay"
        );
        tick(&mut redstone, &mut grid, 4);
        assert!(!property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn comparator_compares_subtracts_and_reports_analog_strength() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], make("target", &[("power", "10")]));
        grid.add(
            [1, 64, 0],
            make(
                "comparator",
                &[
                    ("facing", "west"),
                    ("mode", "compare"),
                    ("powered", "false"),
                ],
            ),
        );
        grid.add([2, 64, 0], wire());
        grid.add([3, 64, 0], lamp());
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("10"));
        assert!(property_bool(grid.state([3, 64, 0]), "lit"));

        let subtract = with_properties(grid.state([1, 64, 0]), &[("mode", "subtract")]).unwrap();
        grid.add([1, 64, 0], subtract);
        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(
            property(grid.state([2, 64, 0]), "power"),
            Some("10"),
            "subtracting a missing side input leaves the input"
        );

        // A powered repeater pointing into the side is a 15 side input.
        grid.add([1, 64, 2], make("target", &[("power", "15")]));
        grid.add(
            [1, 64, 1],
            make(
                "repeater",
                &[("facing", "south"), ("delay", "1"), ("powered", "true")],
            ),
        );
        let compare = with_properties(grid.state([1, 64, 0]), &[("mode", "compare")]).unwrap();
        grid.add([1, 64, 0], compare);
        redstone.blocks_changed(&mut grid, &[[1, 64, 1], [1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("0"));
        tick(&mut redstone, &mut grid, 4);
        assert!(!property_bool(grid.state([3, 64, 0]), "lit"));
    }

    #[test]
    fn comparator_output_subtracts_a_wire_side_input_and_reads_what_it_faces() {
        let mut grid = Grid::new();
        let comparator = |mode: &str| {
            make(
                "comparator",
                &[("facing", "west"), ("mode", mode), ("powered", "false")],
            )
        };
        grid.add([1, 64, 0], comparator("subtract"));
        grid.add([0, 64, 0], make("target", &[("power", "10")]));
        grid.add([1, 64, 1], make("redstone_wire", &[("power", "4")]));
        let analog = HashMap::new();
        let output = |grid: &Grid, mode: &str| {
            Probe::new(grid, &analog).comparator_output([1, 64, 0], comparator(mode))
        };
        assert_eq!(output(&grid, "subtract"), 6, "10 - 4");
        assert_eq!(output(&grid, "compare"), 10, "10 >= 4 keeps the input");
        grid.add([1, 64, 1], make("redstone_wire", &[("power", "12")]));
        assert_eq!(output(&grid, "compare"), 0, "a side input above the input silences it");
        // Behind a conductor the comparator reads the container two blocks away.
        grid.add([1, 64, 1], air());
        grid.add([0, 64, 0], make("stone", &[]));
        grid.add([-1, 64, 0], make("composter", &[("level", "5")]));
        assert_eq!(output(&grid, "compare"), 5);
        // A composter right behind it is read directly.
        grid.add([0, 64, 0], make("composter", &[("level", "3")]));
        assert_eq!(output(&grid, "compare"), 3);
    }

    #[test]
    fn lever_drives_door_gate_rail_note_block_and_tnt() {
        let mut grid = Grid::new();
        let lever_at = |grid: &mut Grid, x: i32| {
            let state = lever();
            grid.add([x, 64, 0], state);
            grid.add([x, 64, 0], use_block(state).unwrap());
        };
        lever_at(&mut grid, 0);
        grid.add(
            [1, 64, 0],
            make(
                "oak_door",
                &[
                    ("half", "lower"),
                    ("facing", "north"),
                    ("open", "false"),
                    ("powered", "false"),
                ],
            ),
        );
        grid.add(
            [1, 65, 0],
            make(
                "oak_door",
                &[
                    ("half", "upper"),
                    ("facing", "north"),
                    ("open", "false"),
                    ("powered", "false"),
                ],
            ),
        );
        lever_at(&mut grid, 2);
        grid.add(
            [3, 64, 0],
            make(
                "oak_fence_gate",
                &[("facing", "north"), ("open", "false"), ("powered", "false")],
            ),
        );
        lever_at(&mut grid, 4);
        grid.add([5, 64, 0], make("powered_rail", &[("powered", "false")]));
        lever_at(&mut grid, 6);
        grid.add([7, 64, 0], make("tnt", &[]));
        lever_at(&mut grid, 8);
        grid.add(
            [9, 64, 0],
            make("note_block", &[("note", "0"), ("powered", "false")]),
        );
        lever_at(&mut grid, 10);
        grid.add(
            [11, 64, 0],
            make(
                "oak_trapdoor",
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("open", "false"),
                    ("powered", "false"),
                ],
            ),
        );
        let mut redstone = Redstone::default();
        redstone.blocks_changed(
            &mut grid,
            &[
                [0, 64, 0],
                [2, 64, 0],
                [4, 64, 0],
                [6, 64, 0],
                [8, 64, 0],
                [10, 64, 0],
            ],
        );
        tick(&mut redstone, &mut grid, 1);

        assert!(property_bool(grid.state([1, 64, 0]), "open"));
        assert!(property_bool(grid.state([1, 65, 0]), "open"));
        assert!(property_bool(grid.state([3, 64, 0]), "open"));
        assert!(property_bool(grid.state([5, 64, 0]), "powered"));
        assert_eq!(name(grid.state([7, 64, 0])), Some("air"));
        assert_eq!(
            redstone.take_primed(),
            vec![([7, 64, 0], ferrite_entity::tnt::FUSE)]
        );
        assert_eq!(
            property(grid.state([9, 64, 0]), "note"),
            Some("0"),
            "`NoteBlock.neighborChanged` plays the note without changing it"
        );
        assert!(property_bool(grid.state([9, 64, 0]), "powered"));
        assert!(property_bool(grid.state([11, 64, 0]), "open"));
    }

    #[test]
    fn observer_emits_a_two_tick_pulse_when_the_block_in_front_changes() {
        let mut grid = Grid::new();
        grid.add(
            [1, 64, 0],
            make("observer", &[("facing", "east"), ("powered", "false")]),
        );
        grid.add([2, 64, 0], make("stone", &[]));
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[2, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(!property_bool(grid.state([1, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([1, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 2);
        assert!(!property_bool(grid.state([1, 64, 0]), "powered"));
    }

    #[test]
    fn a_players_box_follows_the_pose() {
        let standing = Occupant::player_in_pose([0.5, 64.0, 0.5], false, false);
        assert_eq!((standing.half, standing.height), (0.3, 1.8));
        assert_eq!(standing, Occupant::player([0.5, 64.0, 0.5]));
        let sneaking = Occupant::player_in_pose([0.5, 64.0, 0.5], false, true);
        assert_eq!(sneaking.height, 1.5);
        let asleep = Occupant::player_in_pose([0.5, 64.0, 0.5], true, true);
        assert_eq!((asleep.half, asleep.height), (0.1, 0.2));
        // A sleeper's narrow box misses a plate its neighbour's full box would press.
        assert!(standing.touches([1, 64, 0]) || !asleep.touches([1, 64, 0]));
        assert!(!asleep.touches([2, 64, 0]));
    }

    #[test]
    fn pressure_plate_tracks_players_and_daylight_detector_tracks_brightness() {
        let mut grid = Grid::new();
        grid.add(
            [0, 64, 0],
            make("stone_pressure_plate", &[("powered", "false")]),
        );
        grid.add(
            [4, 64, 0],
            make(
                "daylight_detector",
                &[("power", "0"), ("inverted", "false")],
            ),
        );
        grid.add(
            [6, 64, 0],
            make("daylight_detector", &[("power", "0"), ("inverted", "true")]),
        );
        grid.brightness = 12;
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[0, 64, 0], [4, 64, 0], [6, 64, 0]]);
        redstone.tick(&mut grid, &[[0.5, 64.0, 0.5]]);
        assert!(property_bool(grid.state([0, 64, 0]), "powered"));
        assert_eq!(property(grid.state([4, 64, 0]), "power"), Some("12"));
        assert_eq!(property(grid.state([6, 64, 0]), "power"), Some("3"));

        // A pressed plate looks again only 20 ticks after the last look (`getPressedTime`).
        for _ in 0..19 {
            redstone.tick(&mut grid, &[]);
            assert!(property_bool(grid.state([0, 64, 0]), "powered"));
        }
        redstone.tick(&mut grid, &[]);
        assert!(!property_bool(grid.state([0, 64, 0]), "powered"));
    }

    #[test]
    fn plates_sense_items_by_material_and_weighted_plates_count_entities() {
        let item = Occupant {
            pos: [0.5, 64.0, 0.5],
            half: 0.125,
            height: 0.25,
            living: false,
        };
        let mut grid = Grid::new();
        grid.add(
            [0, 64, 0],
            make("stone_pressure_plate", &[("powered", "false")]),
        );
        grid.add(
            [2, 64, 0],
            make("oak_pressure_plate", &[("powered", "false")]),
        );
        grid.add(
            [4, 64, 0],
            make("light_weighted_pressure_plate", &[("power", "0")]),
        );
        grid.add(
            [6, 64, 0],
            make("heavy_weighted_pressure_plate", &[("power", "0")]),
        );
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[0, 64, 0], [2, 64, 0], [4, 64, 0], [6, 64, 0]]);
        let at = |x: f64| Occupant {
            pos: [x + 0.5, 64.0, 0.5],
            ..item
        };
        redstone.set_occupants(vec![at(0.0), at(2.0), at(4.0), at(4.0), at(6.0), at(6.0)]);
        redstone.tick(&mut grid, &[]);
        assert!(
            !property_bool(grid.state([0, 64, 0]), "powered"),
            "stone ignores items"
        );
        assert!(
            property_bool(grid.state([2, 64, 0]), "powered"),
            "wood senses items"
        );
        assert_eq!(
            property(grid.state([4, 64, 0]), "power"),
            Some("2"),
            "two items on gold: ceil(2 / 15 * 15)"
        );
        assert_eq!(
            property(grid.state([6, 64, 0]), "power"),
            Some("1"),
            "two items on iron: ceil(2 / 150 * 15)"
        );
    }

    #[test]
    fn wire_power_decays_one_per_block() {
        let mut grid = Grid::new();
        let source = lever();
        grid.add([0, 64, 0], source);
        for x in 1..=16 {
            grid.add([x, 64, 0], wire());
        }
        let mut redstone = Redstone::default();

        grid.add([0, 64, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        for x in 1..=15 {
            assert_eq!(
                property(grid.state([x, 64, 0]), "power"),
                Some(POWER[16 - usize::try_from(x).unwrap()])
            );
        }
        assert_eq!(property(grid.state([16, 64, 0]), "power"), Some("0"));
    }

    #[test]
    fn strong_power_reaches_wire_and_lamp_through_stone() {
        let mut grid = Grid::new();
        let source = make(
            "lever",
            &[("face", "wall"), ("facing", "west"), ("powered", "false")],
        );
        grid.add([0, 64, 0], source);
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add([1, 65, 0], wire());
        grid.add([2, 64, 0], lamp());
        let mut redstone = Redstone::default();

        grid.add([0, 64, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert_eq!(property(grid.state([1, 65, 0]), "power"), Some("15"));
        assert!(property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn redstone_block_powers_an_adjacent_wire() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], make("redstone_block", &[]));
        grid.add([1, 64, 0], wire());
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert_eq!(property(grid.state([1, 64, 0]), "power"), Some("15"));
    }

    #[test]
    fn buttons_release_after_their_press_lengths() {
        let mut grid = Grid::new();
        let stone = make(
            "stone_button",
            &[("face", "floor"), ("facing", "north"), ("powered", "false")],
        );
        let wood = make(
            "oak_button",
            &[("face", "floor"), ("facing", "north"), ("powered", "false")],
        );
        grid.add([0, 64, 0], use_block(stone).unwrap());
        grid.add([2, 64, 0], use_block(wood).unwrap());
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[0, 64, 0], [2, 64, 0]]);
        tick(&mut redstone, &mut grid, 19);
        assert!(property_bool(grid.state([0, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 1);
        assert!(!property_bool(grid.state([0, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 9);
        assert!(property_bool(grid.state([2, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 1);
        assert!(!property_bool(grid.state([2, 64, 0]), "powered"));
    }

    #[test]
    fn a_powered_repeater_is_a_side_input_to_a_comparator_not_a_lock() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], make("target", &[("power", "10")]));
        grid.add(
            [1, 64, 0],
            make(
                "comparator",
                &[
                    ("facing", "west"),
                    ("mode", "compare"),
                    ("powered", "false"),
                ],
            ),
        );
        grid.add([2, 64, 0], wire());
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("10"));

        // `ComparatorBlock` has no lock: a powered repeater pointing into its side is a 15
        // side input, so compare mode (side > input) outputs nothing.
        grid.add([1, 64, 2], make("target", &[("power", "15")]));
        grid.add(
            [1, 64, 1],
            make(
                "repeater",
                &[("facing", "south"), ("delay", "1"), ("powered", "true")],
            ),
        );
        redstone.blocks_changed(&mut grid, &[[1, 64, 1]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("0"));

        // The input is still read while the side input is there: a stronger input wins.
        grid.add([0, 64, 0], make("target", &[("power", "15")]));
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("15"));

        grid.add([0, 64, 0], make("target", &[("power", "4")]));
        grid.add([1, 64, 1], air());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0], [1, 64, 1]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("4"));
    }

    #[test]
    fn repeater_holds_its_state_while_locked() {
        let mut grid = Grid::new();
        let source = lever();
        grid.add([0, 64, 0], source);
        grid.add(
            [1, 64, 0],
            make(
                "repeater",
                &[("facing", "west"), ("delay", "1"), ("powered", "false")],
            ),
        );
        let mut redstone = Redstone::default();

        let on = use_block(source).unwrap();
        grid.add([0, 64, 0], on);
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert!(property_bool(grid.state([1, 64, 0]), "powered"));

        grid.add([1, 64, 2], make("target", &[("power", "15")]));
        grid.add(
            [1, 64, 1],
            make(
                "repeater",
                &[("facing", "south"), ("delay", "1"), ("powered", "true")],
            ),
        );
        redstone.blocks_changed(&mut grid, &[[1, 64, 1]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([1, 64, 0]), "locked"), Some("true"));

        let off = use_block(on).unwrap();
        grid.add([0, 64, 0], off);
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 4);
        assert!(property_bool(grid.state([1, 64, 0]), "powered"));

        grid.add([1, 64, 1], air());
        redstone.blocks_changed(&mut grid, &[[1, 64, 1]]);
        tick(&mut redstone, &mut grid, 4);
        assert!(!property_bool(grid.state([1, 64, 0]), "powered"));
    }

    #[test]
    fn wire_connects_up_to_a_raised_wire_and_side_to_components() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], wire());
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add([1, 65, 0], wire());
        grid.add(
            [0, 64, 1],
            make(
                "lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "false")],
            ),
        );
        let mut redstone = Redstone::default();

        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        let state = grid.state([0, 64, 0]);
        assert_eq!(property(state, "east"), Some("up"));
        assert_eq!(property(state, "south"), Some("side"));
    }

    #[test]
    fn torch_burns_out_after_repeated_toggles_and_recovers() {
        let mut grid = Grid::new();
        grid.add([1, 63, 0], make("stone", &[]));
        grid.add([1, 64, 0], make("redstone_torch", &[("lit", "true")]));
        let mut source = make(
            "lever",
            &[("face", "wall"), ("facing", "west"), ("powered", "false")],
        );
        grid.add([0, 63, 0], source);
        let mut redstone = Redstone::default();

        // Seven turn-offs are fine; the eighth within 60 ticks burns the torch out.
        for _ in 0..7 {
            source = use_block(source).unwrap();
            grid.add([0, 63, 0], source);
            redstone.blocks_changed(&mut grid, &[[0, 63, 0]]);
            tick(&mut redstone, &mut grid, 2);
            assert!(!property_bool(grid.state([1, 64, 0]), "lit"));

            source = use_block(source).unwrap();
            grid.add([0, 63, 0], source);
            redstone.blocks_changed(&mut grid, &[[0, 63, 0]]);
            tick(&mut redstone, &mut grid, 2);
            assert!(property_bool(grid.state([1, 64, 0]), "lit"));
        }

        source = use_block(source).unwrap();
        grid.add([0, 63, 0], source);
        redstone.blocks_changed(&mut grid, &[[0, 63, 0]]);
        tick(&mut redstone, &mut grid, 2);
        source = use_block(source).unwrap();
        grid.add([0, 63, 0], source);
        redstone.blocks_changed(&mut grid, &[[0, 63, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert!(!property_bool(grid.state([1, 64, 0]), "lit"));

        tick(&mut redstone, &mut grid, 160);
        assert!(property_bool(grid.state([1, 64, 0]), "lit"));
    }

    fn on_lever(face: &str, facing: &str) -> u16 {
        make(
            "lever",
            &[("face", face), ("facing", facing), ("powered", "true")],
        )
    }

    #[test]
    fn repeater_stretches_a_pulse_shorter_than_its_delay() {
        let mut grid = Grid::new();
        let off = lever();
        grid.add([0, 64, 0], off);
        grid.add(
            [1, 64, 0],
            make(
                "repeater",
                &[("facing", "west"), ("delay", "1"), ("powered", "false")],
            ),
        );
        grid.add([2, 64, 0], lamp());
        let mut redstone = Redstone::default();

        // One tick of input: gone again before the repeater's two-tick delay is up.
        grid.add([0, 64, 0], use_block(off).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        grid.add([0, 64, 0], off);
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(
            property_bool(grid.state([1, 64, 0]), "powered"),
            "`DiodeBlock.tick` turns on even though the input is already gone"
        );
        assert!(property_bool(grid.state([2, 64, 0]), "lit"));
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([1, 64, 0]), "powered"));
        tick(&mut redstone, &mut grid, 1);
        assert!(
            !property_bool(grid.state([1, 64, 0]), "powered"),
            "and turns off after one more delay"
        );
    }

    #[test]
    fn repeater_and_observer_power_only_the_block_in_front_of_them() {
        let mut grid = Grid::new();
        // A repeater facing west takes input from the west and outputs east.
        grid.add(
            [1, 64, 0],
            make(
                "repeater",
                &[("facing", "west"), ("delay", "1"), ("powered", "true")],
            ),
        );
        grid.add([0, 64, 0], make("redstone_block", &[]));
        grid.add([2, 64, 0], lamp());
        grid.add([1, 64, 1], lamp());
        grid.add([1, 64, -1], lamp());
        // An observer facing east watches the east and outputs west.
        grid.add(
            [11, 64, 0],
            make("observer", &[("facing", "east"), ("powered", "true")]),
        );
        grid.add([10, 64, 0], lamp());
        grid.add([12, 64, 0], lamp());
        grid.add([11, 64, 1], lamp());
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[1, 64, 0], [11, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([2, 64, 0]), "lit"), "repeater front");
        assert!(!property_bool(grid.state([1, 64, 1]), "lit"), "repeater side");
        assert!(!property_bool(grid.state([1, 64, -1]), "lit"), "repeater side");
        assert!(property_bool(grid.state([10, 64, 0]), "lit"), "observer back");
        assert!(!property_bool(grid.state([12, 64, 0]), "lit"), "observer face");
        assert!(!property_bool(grid.state([11, 64, 1]), "lit"), "observer side");
    }

    #[test]
    fn a_torch_powers_the_block_above_it_but_not_the_one_it_stands_on() {
        let mut grid = Grid::new();
        grid.add([1, 63, 0], make("stone", &[]));
        grid.add([1, 64, 0], make("redstone_torch", &[("lit", "true")]));
        grid.add([1, 65, 0], make("stone", &[]));
        grid.add([2, 65, 0], lamp());
        grid.add([1, 63, 1], lamp());
        grid.add([1, 62, 0], lamp());
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(
            property_bool(grid.state([2, 65, 0]), "lit"),
            "`getDirectSignal(DOWN)` strongly powers the block above"
        );
        assert!(
            !property_bool(grid.state([1, 62, 0]), "lit"),
            "the support block is not powered by its own torch"
        );
        assert!(
            property_bool(grid.state([1, 64, 0]), "lit"),
            "so the torch does not turn itself off"
        );
    }

    #[test]
    fn wire_networks_follow_staircases_and_dots_do_not_power_sideways() {
        let mut grid = Grid::new();
        let source = lever();
        grid.add([-1, 64, 0], source);
        grid.add([0, 64, 0], wire());
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add([1, 65, 0], wire());
        grid.add([2, 64, 0], wire());
        let mut redstone = Redstone::default();
        grid.add([-1, 64, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[-1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([0, 64, 0]), "power"), Some("15"));
        assert_eq!(
            property(grid.state([1, 65, 0]), "power"),
            Some("14"),
            "up over the conductor"
        );
        assert_eq!(
            property(grid.state([2, 64, 0]), "power"),
            Some("13"),
            "and down again"
        );
        assert_eq!(property(grid.state([0, 64, 0]), "east"), Some("up"));

        // A wire on a redstone block with nothing beside it is a dot: lamps next to it stay dark.
        let mut grid = Grid::new();
        grid.add([1, 63, 0], make("redstone_block", &[]));
        grid.add([1, 64, 0], wire());
        grid.add([2, 64, 0], lamp());
        grid.add([1, 63, 1], lamp());
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[1, 64, 0]]);
        tick(&mut redstone, &mut grid, 6);
        let dot = grid.state([1, 64, 0]);
        assert_eq!(property(dot, "power"), Some("15"));
        assert_eq!(property(dot, "east"), Some("none"));
        assert!(!property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn wire_does_not_connect_to_plain_blocks_or_lamps() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], wire());
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add([-1, 64, 0], lamp());
        grid.add([0, 64, 1], make("repeater", &[("facing", "north"), ("delay", "1")]));
        grid.add([0, 64, -1], make("repeater", &[("facing", "east"), ("delay", "1")]));
        let shape = wire_shape(&grid, [0, 64, 0], grid.state([0, 64, 0]));
        // East (stone) and west (lamp) stay unconnected; the repeater in line to the south
        // connects, and the lone line extends north although the repeater there points across.
        assert_eq!(shape, ["side", "none", "side", "none"]);
        grid.add([0, 64, -1], air());
        let shape = wire_shape(&grid, [0, 64, 0], grid.state([0, 64, 0]));
        assert_eq!(shape, ["side", "none", "side", "none"]);
        assert_eq!(
            connecting_side(&grid, [0, 64, 0], Direction::North, true),
            "none",
            "nothing north of it connects by itself"
        );
    }

    #[test]
    fn either_half_of_a_door_feels_a_signal() {
        let mut grid = Grid::new();
        let door = |half: &str| {
            make(
                "oak_door",
                &[
                    ("half", half),
                    ("facing", "north"),
                    ("open", "false"),
                    ("powered", "false"),
                ],
            )
        };
        grid.add([1, 64, 0], door("lower"));
        grid.add([1, 65, 0], door("upper"));
        grid.add([0, 65, 0], on_lever("wall", "west"));
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[0, 65, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert!(property_bool(grid.state([1, 64, 0]), "open"));
        assert!(property_bool(grid.state([1, 65, 0]), "open"));
    }

    /// Scheduled redstone ticks are absolute in their region's clock. A merge from a region
    /// with a different clock must translate them by their remaining delay.
    #[test]
    fn merge_keeps_remaining_delay_across_different_region_clocks() {
        let mut left = Redstone {
            clock: 100,
            ..Redstone::default()
        };
        let mut right = Redstone {
            clock: 10,
            ..Redstone::default()
        };
        right.schedule([1, 64, 0], Kind::Diode, 3);
        left.merge(right);
        assert_eq!(left.scheduled[&([1, 64, 0], Kind::Diode)], 103);
    }

    #[test]
    fn split_and_merge_keep_scheduled_ticks_with_their_sections() {
        let mut redstone = Redstone::default();
        redstone.schedule([0, 64, 0], Kind::Diode, 4);
        redstone.schedule([32, 64, 0], Kind::Diode, 4);
        let mut moved = redstone.split(|x, _| x.div_euclid(16) < 1);
        moved.merge(Redstone::default());
        assert!(moved.scheduled.contains_key(&([0, 64, 0], Kind::Diode)));
        assert!(!moved.scheduled.contains_key(&([32, 64, 0], Kind::Diode)));
        assert!(redstone.scheduled.contains_key(&([32, 64, 0], Kind::Diode)));
        assert!(!redstone.scheduled.contains_key(&([0, 64, 0], Kind::Diode)));
    }

    fn torch_world() -> (Grid, Redstone) {
        let mut grid = Grid::new();
        grid.add([1, 63, 0], make("stone", &[]));
        grid.add([1, 64, 0], make("redstone_torch", &[("lit", "true")]));
        grid.add(
            [0, 63, 0],
            make(
                "lever",
                &[("face", "wall"), ("facing", "west"), ("powered", "true")],
            ),
        );
        (grid, Redstone::default())
    }

    /// `RedstoneTorchBlock.isToggledTooFrequently`: toggles older than 60 ticks (`clock -
    /// first > 60`) are forgotten, exactly 60 still count, and only turning off records one.
    #[test]
    fn torch_burnout_window_is_sixty_ticks_and_counts_turn_offs() {
        let (mut grid, mut redstone) = torch_world();
        redstone.clock = 100;
        redstone.toggles.insert([1, 64, 0], VecDeque::from(vec![40; 7]));
        redstone.update_torch(&mut grid, [1, 64, 0]);
        assert!(!property_bool(grid.state([1, 64, 0]), "lit"));
        assert!(
            redstone.scheduled.contains_key(&([1, 64, 0], Kind::Torch)),
            "the eighth toggle within the window schedules the 160 tick relight"
        );

        let (mut grid, mut redstone) = torch_world();
        redstone.clock = 100;
        redstone.toggles.insert([1, 64, 0], VecDeque::from(vec![39; 7]));
        redstone.update_torch(&mut grid, [1, 64, 0]);
        assert!(!property_bool(grid.state([1, 64, 0]), "lit"));
        assert!(
            !redstone.scheduled.contains_key(&([1, 64, 0], Kind::Torch)),
            "toggles 61 ticks old no longer count"
        );
        assert_eq!(redstone.toggles[&[1, 64, 0]].len(), 1);
    }

    fn rail(powered: bool, shape: &str) -> u16 {
        make(
            "powered_rail",
            &[
                ("powered", if powered { "true" } else { "false" }),
                ("shape", shape),
            ],
        )
    }

    /// `PoweredRailBlock.findPoweredRailSignal`: a powered rail powers rails along its axis,
    /// up to eight beyond the one that receives the signal, and no further.
    #[test]
    fn a_powered_rail_line_carries_power_eight_rails_along_the_track() {
        let mut grid = Grid::new();
        for x in 1..=10 {
            grid.add([x, 64, 0], rail(false, "east_west"));
        }
        grid.add(
            [0, 64, 0],
            make(
                "lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "true")],
            ),
        );
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 6);
        for x in 1..=9 {
            assert!(
                property_bool(grid.state([x, 64, 0]), "powered"),
                "rail {x} is within reach"
            );
        }
        assert!(
            !property_bool(grid.state([10, 64, 0]), "powered"),
            "the tenth rail is one too far"
        );

        // Cutting the source unpowers the whole line again.
        grid.add(
            [0, 64, 0],
            make(
                "lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "false")],
            ),
        );
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 12);
        for x in 1..=10 {
            assert!(!property_bool(grid.state([x, 64, 0]), "powered"), "rail {x}");
        }
    }

    #[test]
    fn a_powered_rail_does_not_carry_power_across_a_perpendicular_track() {
        let mut grid = Grid::new();
        grid.add([1, 64, 0], rail(false, "east_west"));
        grid.add([2, 64, 0], rail(false, "north_south"));
        grid.add(
            [0, 64, 0],
            make(
                "lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "true")],
            ),
        );
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 4);
        assert!(property_bool(grid.state([1, 64, 0]), "powered"));
        assert!(!property_bool(grid.state([2, 64, 0]), "powered"));
    }

    #[test]
    fn detector_rail_tripwire_hook_and_trapped_chest_emit_like_vanilla() {
        let detector = make("detector_rail", &[("powered", "true"), ("shape", "east_west")]);
        let state = detector;
        assert_eq!(signal_toward(state, 0, Direction::West, true), 15);
        assert_eq!(direct_toward(state, 0, Direction::Up, true), 15);
        assert_eq!(direct_toward(state, 0, Direction::North, true), 0);
        let off = make("detector_rail", &[("powered", "false"), ("shape", "east_west")]);
        assert_eq!(direct_toward(off, 0, Direction::Up, true), 0);

        let hook = make(
            "tripwire_hook",
            &[("facing", "north"), ("powered", "true"), ("attached", "true")],
        );
        assert_eq!(signal_toward(hook, 0, Direction::East, true), 15);
        assert_eq!(direct_toward(hook, 0, Direction::North, true), 15);
        assert_eq!(direct_toward(hook, 0, Direction::South, true), 0);

        let chest = make("trapped_chest", &[("facing", "north"), ("type", "single")]);
        assert_eq!(signal_toward(chest, 0, Direction::Up, true), 0);
        assert_eq!(signal_toward(chest, 4, Direction::East, true), 4);
        assert_eq!(signal_toward(chest, 20, Direction::East, true), 15);
        assert_eq!(direct_toward(chest, 4, Direction::Up, true), 4);
        assert_eq!(direct_toward(chest, 4, Direction::East, true), 0);
    }

    /// A trapped chest powers the block beneath it strongly (`getDirectSignal` only towards
    /// up, the way a pressure plate does), so a lamp under that block lights while it is open.
    #[test]
    fn trapped_chest_viewers_power_a_lamp_below_through_direct_signal() {
        let mut grid = Grid::new();
        grid.add(
            [0, 66, 0],
            make("trapped_chest", &[("facing", "north"), ("type", "single")]),
        );
        grid.add([0, 65, 0], make("stone", &[]));
        grid.add([0, 64, 0], lamp());
        let mut redstone = Redstone::default();
        redstone.set_chest_viewers(&HashMap::from([([0, 66, 0], 1u8)]));
        tick(&mut redstone, &mut grid, 2);
        assert!(property_bool(grid.state([0, 64, 0]), "lit"));
        redstone.set_chest_viewers(&HashMap::new());
        tick(&mut redstone, &mut grid, 6);
        assert!(!property_bool(grid.state([0, 64, 0]), "lit"));
        assert!(!redstone.has_chests());
    }

    /// `ComparatorBlock.getItemFrame`: through a conductor the comparator reads the frame
    /// behind it, `rotation % 8 + 1` when it holds an item and 0 when empty.
    #[test]
    fn comparator_reads_an_item_frame_behind_a_conductor() {
        let mut grid = Grid::new();
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add(
            [2, 64, 0],
            make(
                "comparator",
                &[
                    ("facing", "west"),
                    ("mode", "compare"),
                    ("powered", "false"),
                ],
            ),
        );
        grid.add([3, 64, 0], wire());
        let mut redstone = Redstone::default();
        redstone.set_item_frames(&[([0, 64, 0], Direction::West, 4)]);
        redstone.blocks_changed(&mut grid, &[[2, 64, 0]]);
        tick(&mut redstone, &mut grid, 4);
        assert_eq!(property(grid.state([3, 64, 0]), "power"), Some("4"));

        // The frame turned: its output changes and the comparator follows.
        redstone.set_item_frames(&[([0, 64, 0], Direction::West, 8)]);
        tick(&mut redstone, &mut grid, 4);
        assert_eq!(property(grid.state([3, 64, 0]), "power"), Some("8"));

        // A frame facing another way, or two on one face, is not what the comparator reads.
        redstone.set_item_frames(&[([0, 64, 0], Direction::East, 8)]);
        tick(&mut redstone, &mut grid, 4);
        assert_eq!(property(grid.state([3, 64, 0]), "power"), Some("0"));
        redstone.set_item_frames(&[
            ([0, 64, 0], Direction::West, 8),
            ([0, 64, 0], Direction::West, 3),
        ]);
        tick(&mut redstone, &mut grid, 4);
        assert_eq!(property(grid.state([3, 64, 0]), "power"), Some("0"));

        redstone.set_item_frames(&[]);
        assert!(!redstone.has_frames());
    }

    /// Vanilla orders ticks due together by priority and then insertion order.
    #[test]
    fn scheduled_ticks_with_equal_time_and_priority_run_in_insertion_order() {
        let mut redstone = Redstone::default();
        // Positions deliberately sort against the insertion order.
        let order = [[9, 64, 0], [3, 64, 0], [7, 64, 0], [1, 64, 0]];
        for pos in order {
            redstone.schedule(pos, Kind::Diode, 2);
        }
        redstone.clock = 2;
        let mut due: Vec<_> = redstone
            .scheduled
            .keys()
            .map(|&(pos, kind)| (redstone.sequence[&(pos, kind)], pos))
            .collect();
        due.sort();
        assert_eq!(due.iter().map(|(_, pos)| *pos).collect::<Vec<_>>(), order);

        // Splitting and merging keeps the relative order of the moved ticks.
        let mut moved = redstone.split(|x, _| x < 8);
        let mut target = Redstone::default();
        target.schedule([100, 64, 0], Kind::Diode, 2);
        target.merge(moved.split(|_, _| true));
        let sequence = |pos| target.sequence[&(pos, Kind::Diode)];
        assert!(sequence([100, 64, 0]) < sequence([3, 64, 0]));
        assert!(sequence([3, 64, 0]) < sequence([1, 64, 0]));
    }

    /// A conductor on this side carrying strong power to a block the neighbouring region
    /// owns wakes that block, and an observer watching the changed block across the
    /// boundary pulses.
    #[test]
    fn boundary_circuits_wake_the_other_region() {
        let mut left = Edge::new(true);
        left.blocks.insert([14, 64, 0], make("stone", &[]));
        left.blocks.insert([15, 64, 0], make("stone", &[]));
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut left, &[[14, 64, 0]]);
        assert!(
            left.wakes.contains(&([16, 64, 0], [14, 64, 0])),
            "the block behind the carrier conductor is woken: {:?}",
            left.wakes
        );

        let mut right = Edge::new(false);
        right.blocks.insert(
            [16, 64, 0],
            make("observer", &[("facing", "west"), ("powered", "false")]),
        );
        let mut redstone = Redstone::default();
        redstone.foreign_changed(&mut right, [16, 64, 0], [15, 64, 0]);
        assert!(redstone.scheduled.contains_key(&([16, 64, 0], Kind::ObserverPulse)));
        // An observer facing away is not looking at the change.
        let mut other = Redstone::default();
        right.blocks.insert(
            [16, 64, 0],
            make("observer", &[("facing", "east"), ("powered", "false")]),
        );
        other.foreign_changed(&mut right, [16, 64, 0], [15, 64, 0]);
        assert!(other.scheduled.is_empty());
    }

    /// A repeater at the edge must react to its input across the boundary: the owning
    /// region evaluates it from the foreign read, then its own change wakes the other side.
    #[test]
    fn a_repeater_next_to_the_boundary_reacts_and_wakes_back() {
        let mut right = Edge::new(false);
        right.blocks.insert(
            [16, 64, 0],
            make(
                "repeater",
                &[
                    ("facing", "west"),
                    ("delay", "1"),
                    ("powered", "false"),
                    ("locked", "false"),
                ],
            ),
        );
        right.blocks.insert(
            [15, 64, 0],
            make(
                "lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "true")],
            ),
        );
        let mut redstone = Redstone::default();
        redstone.foreign_changed(&mut right, [16, 64, 0], [15, 64, 0]);
        for _ in 0..4 {
            redstone.tick(&mut right, &[]);
        }
        assert!(property_bool(right.state([16, 64, 0]), "powered"));
        assert!(
            right.wakes.iter().any(|(pos, _)| pos[0] == 15),
            "the repeater's own change wakes the region that owns x=15"
        );
    }
}
