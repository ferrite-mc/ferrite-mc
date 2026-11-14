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
/// Vanilla burnout lasts until the torch is updated again; a conservative 8 s.
const TORCH_BURNOUT_TICKS: u64 = 160;
/// Vanilla button press lengths: stone 1 s, wooden 1.5 s.
const STONE_BUTTON_TICKS: u64 = 20;
const WOOD_BUTTON_TICKS: u64 = 30;

const POWER: [&str; 16] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
];
/// Vanilla note block `note` property range (0..=24).
const NOTE: [&str; 25] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24",
];

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

    /// Ask the regions owning `positions` to re-evaluate redstone there. The default is a
    /// no-op for local worlds and tests.
    fn wake_redstone(&mut self, _positions: &[[i32; 3]]) {}

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

    fn wake_redstone(&mut self, positions: &[[i32; 3]]) {
        let Some(signals) = &self.signals else {
            return;
        };
        let positions: Vec<_> = positions
            .iter()
            .copied()
            .filter(|pos| self.source.block(pos[0], pos[1], pos[2]).is_some())
            .collect();
        if !positions.is_empty() {
            let _ = signals.send(Signal::RedstoneChange {
                dimension: self.source.dimension(),
                positions,
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

/// Redstone components are not conductive even when their model is a full cube, matching
/// vanilla `BlockState.isRedstoneConductor` overrides.
fn is_component(name: &str) -> bool {
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
            | "redstone_lamp"
            | "note_block"
            | "tnt"
            | "powered_rail"
            | "piston"
            | "sticky_piston"
    ) || name.ends_with("_button")
        || name.ends_with("_pressure_plate")
        || name.ends_with("_door")
        || name.ends_with("_trapdoor")
        || name.ends_with("_fence_gate")
}

/// Vanilla `BlockState.isRedstoneConductor`: a full, opaque block that carries strong power.
fn is_conductor(state: u16) -> bool {
    name(state).is_some_and(|name| !is_component(name))
        && ferrite_data::block_props::get(state).can_occlude
}

/// Direct (strong) signal a block state emits (`BlockState.getDirectSignal`). Comparators carry
/// their computed analog strength in [`Redstone::analog`], passed in by the caller.
fn direct_signal(state: u16, analog: Option<u8>) -> u8 {
    match name(state) {
        Some("redstone_block") => 15,
        Some("redstone_torch" | "redstone_wall_torch") => {
            if property_bool(state, "lit") {
                15
            } else {
                0
            }
        }
        Some("lever") => {
            if property_bool(state, "powered") {
                15
            } else {
                0
            }
        }
        Some("observer") => {
            if property_bool(state, "powered") {
                15
            } else {
                0
            }
        }
        Some("repeater") => {
            if property_bool(state, "powered") {
                15
            } else {
                0
            }
        }
        Some("comparator") => analog.unwrap_or(0),
        Some("target" | "daylight_detector") => property_u8(state, "power").unwrap_or(0),
        Some("light_weighted_pressure_plate" | "heavy_weighted_pressure_plate") => {
            property_u8(state, "power").unwrap_or(0)
        }
        Some("redstone_wire") => property_u8(state, "power").unwrap_or(0),
        Some(name)
            if (name.ends_with("_button") || name.ends_with("_pressure_plate"))
                && property_bool(state, "powered") =>
        {
            15
        }
        Some(name) if name.ends_with("_button") || name.ends_with("_pressure_plate") => 0,
        _ => 0,
    }
}

/// Weak signal a source emits towards a neighbour (`BlockState.getSignal`). A conductive block
/// emits the strong signal it carries instead; [`power_from`] handles that case.
fn weak_signal(state: u16, analog: Option<u8>) -> u8 {
    if is_conductor(state) {
        return 0;
    }
    direct_signal(state, analog)
}

/// Strong power carried by the conductive block at `pos`: the largest direct signal of any
/// neighbour. Sources are read directly, so this never recurses through another conductor.
fn strong_block_power(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    analog: &HashMap<[i32; 3], u8>,
) -> u8 {
    let mut power = 0;
    for direction in Direction::ALL {
        let neighbour = offset(pos, direction);
        let Some(state) = world.signal_block(neighbour) else {
            continue;
        };
        let signal = if is_comparator(state) {
            analog.get(&neighbour).copied().unwrap_or(0)
        } else {
            direct_signal(state, None)
        };
        power = power.max(signal);
    }
    power
}

/// Power arriving at `pos` from `direction` (`Level.getSignal` through the neighbour).
fn power_from(
    world: &impl RedstoneWorld,
    pos: [i32; 3],
    direction: Direction,
    analog: &HashMap<[i32; 3], u8>,
) -> u8 {
    let neighbour = offset(pos, direction);
    let Some(state) = world.signal_block(neighbour) else {
        return 0;
    };
    if is_wire(state) {
        return property_u8(state, "power").unwrap_or(0);
    }
    if let Some(signal) = world.container_signal(neighbour) {
        return signal;
    }
    if is_conductor(state) {
        return strong_block_power(world, neighbour, analog);
    }
    if is_comparator(state) {
        return analog.get(&neighbour).copied().unwrap_or(0);
    }
    weak_signal(state, None)
}

/// Every direction an adjacent block can power `pos` from.
fn input_power(world: &impl RedstoneWorld, pos: [i32; 3], analog: &HashMap<[i32; 3], u8>) -> u8 {
    Direction::ALL
        .iter()
        .map(|&direction| power_from(world, pos, direction, analog))
        .max()
        .unwrap_or(0)
}

fn redstone_part(name: &str) -> bool {
    is_component(name)
        || name.ends_with("_button")
        || name.ends_with("_pressure_plate")
        || name.ends_with("_door")
        || name.ends_with("_trapdoor")
        || name.ends_with("_fence_gate")
}

/// Wire connection property (`RedStoneWireBlock.getConnectionState`): `side` to an adjacent
/// wire or component, `up` when the wire climbs to the block above the neighbour, `none`.
fn connection(world: &impl RedstoneWorld, pos: [i32; 3], direction: Direction) -> &'static str {
    let neighbour = offset(pos, direction);
    let Some(state) = world.block(neighbour) else {
        return "none";
    };
    if is_wire(state) {
        return "side";
    }
    if world
        .block(offset(neighbour, Direction::Up))
        .is_some_and(is_wire)
        && !is_conductor(state)
    {
        return "up";
    }
    if name(state).is_some_and(redstone_part) || is_conductor(state) {
        return "side";
    }
    "none"
}

fn with_wire_shape(
    state: u16,
    power: u8,
    world: &impl RedstoneWorld,
    pos: [i32; 3],
) -> Option<u16> {
    let mut changes = Vec::with_capacity(5);
    changes.push(("power", POWER[power as usize]));
    for direction in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let key = match direction {
            Direction::North => "north",
            Direction::East => "east",
            Direction::South => "south",
            _ => "west",
        };
        changes.push((key, connection(world, pos, direction)));
    }
    with_properties(state, &changes)
}

/// Connected wire component around `start`, bounded by [`WIRE_LIMIT`].
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
        for direction in Direction::ALL {
            let neighbour = offset(pos, direction);
            if !seen.contains(&neighbour) && world.block(neighbour).is_some_and(is_wire) {
                queue.push_back(neighbour);
            }
        }
    }
    wires
}

/// Recompute a wire component from its non-wire sources, then relax power along the wires.
/// This is `RedStoneWireBlock.calculateTargetStrength` applied until the network settles.
fn wire_powers(
    world: &impl RedstoneWorld,
    wires: &[[i32; 3]],
    analog: &HashMap<[i32; 3], u8>,
) -> HashMap<[i32; 3], u8> {
    let members: HashSet<[i32; 3]> = wires.iter().copied().collect();
    let mut power: HashMap<[i32; 3], u8> = wires.iter().map(|&pos| (pos, 0)).collect();
    let mut queue = VecDeque::new();
    for &pos in wires {
        let mut target = 0;
        // `getBlockSignal`: wire on a conductive block reads the strong power it carries.
        let below = offset(pos, Direction::Down);
        if let Some(state) = world.signal_block(below) {
            if is_conductor(state) {
                target = target.max(strong_block_power(world, below, analog));
            } else {
                target = target.max(weak_signal(state, analog.get(&below).copied()));
            }
        }
        for direction in Direction::ALL {
            let neighbour = offset(pos, direction);
            if members.contains(&neighbour) {
                continue;
            }
            let Some(state) = world.signal_block(neighbour) else {
                continue;
            };
            if is_wire(state) {
                // A wire across the boundary feeds this component with vanilla attenuation.
                // Its own region keeps its power up to date and wakes this one on changes.
                target = target.max(property_u8(state, "power").unwrap_or(0).saturating_sub(1));
            } else if is_conductor(state) {
                target = target.max(strong_block_power(world, neighbour, analog));
            } else {
                let analog = if is_comparator(state) {
                    analog.get(&neighbour).copied()
                } else {
                    None
                };
                target = target.max(weak_signal(state, analog));
            }
        }
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
        for direction in Direction::ALL {
            let neighbour = offset(pos, direction);
            if !members.contains(&neighbour) {
                continue;
            }
            if power[&neighbour] < strength - 1 {
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
}

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
    dirty: VecDeque<[i32; 3]>,
    wires: HashSet<[i32; 3]>,
    /// Computed comparator strength per position (the state only carries `powered`).
    analog: HashMap<[i32; 3], u8>,
    /// Recent off->on transitions for `RedstoneTorchBlock` burnout.
    toggles: HashMap<[i32; 3], VecDeque<u64>>,
    plates: HashSet<[i32; 3]>,
    daylights: HashSet<[i32; 3]>,
    primed: Vec<([i32; 3], i32)>,
    piston_candidates: HashSet<[i32; 3]>,
}

impl Redstone {
    fn schedule(&mut self, pos: [i32; 3], kind: Kind, delay: u64) {
        let due = self.clock + delay;
        self.scheduled
            .entry((pos, kind))
            .and_modify(|current| *current = (*current).min(due))
            .or_insert(due);
    }

    /// `Level.markAndNotifyBlock`: notify the changed block and its six neighbours.
    pub fn blocks_changed(&mut self, world: &mut impl RedstoneWorld, positions: &[[i32; 3]]) {
        for &pos in positions {
            self.after_change(world, pos, true);
        }
    }

    /// Re-evaluates a change another region made across the boundary. It never wakes back to
    /// the source, so two regions cannot ping-pong the same edge forever.
    pub fn foreign_changed(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        self.after_change(world, pos, false);
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
        let due: Vec<([i32; 3], Kind)> = self
            .scheduled
            .iter()
            .filter(|&(_, &time)| time <= self.clock)
            .map(|(&key, _)| key)
            .take(UPDATE_LIMIT)
            .collect();
        for (pos, kind) in due {
            self.scheduled.remove(&(pos, kind));
            match kind {
                Kind::Diode => self.update_diode(world, pos),
                Kind::Torch => self.update_torch(world, pos),
                Kind::Button => self.release_button(world, pos),
                Kind::ObserverPulse => self.pulse_observer(world, pos),
                Kind::ObserverOff => self.unpulse_observer(world, pos),
                Kind::Daylight => self.update_daylight(world, pos),
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
                    if world.block(candidate).is_some_and(|state| {
                        matches!(name(state), Some("piston" | "sticky_piston"))
                    }) {
                        self.piston_candidates.insert(candidate);
                    }
                }
            }
        }
        if world.block(pos).is_some_and(is_wire) {
            self.wires.insert(pos);
        } else if let Some(state) = world.block(pos) {
            self.update_component(world, pos, state);
        }
        for direction in Direction::ALL {
            let neighbour = offset(pos, direction);
            let Some(state) = world.block(neighbour) else {
                if wake {
                    wakes.push(neighbour);
                }
                continue;
            };
            if is_wire(state) {
                self.wires.insert(neighbour);
            } else if is_observer(state) && facing(state) == Some(direction.opposite()) {
                // `ObserverBlock.updateShape`: the observer saw a block change in front.
                self.schedule(neighbour, Kind::ObserverPulse, 2);
            } else {
                self.update_component(world, neighbour, state);
            }
        }
        // Strong power reaches components through one conductive block. A lever on a wall
        // powers the stone behind it, which in turn powers the torch standing on that stone.
        for direction in Direction::ALL {
            let carrier = offset(pos, direction);
            let Some(carrier_state) = world.block(carrier) else {
                if wake {
                    wakes.push(carrier);
                }
                continue;
            };
            if !is_conductor(carrier_state) {
                continue;
            }
            for inner in Direction::ALL {
                let target = offset(carrier, inner);
                if target == pos {
                    continue;
                }
                if let Some(state) = world.block(target) {
                    if is_wire(state) {
                        self.wires.insert(target);
                    } else if !is_conductor(state) {
                        self.update_component(world, target, state);
                    }
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
                let delay = u64::from(property_u8(state, "delay").unwrap_or(1)) * 2;
                self.schedule(pos, Kind::Diode, delay);
                self.update_lock(world, pos, state);
            }
            "comparator" => self.schedule(pos, Kind::Diode, 2),
            "redstone_torch" | "redstone_wall_torch" => self.schedule(pos, Kind::Torch, 2),
            "redstone_lamp" => {
                let lit = input_power(world, pos, &self.analog) > 0;
                if lit != property_bool(state, "lit")
                    && let Some(next) =
                        with_properties(state, &[("lit", if lit { "true" } else { "false" })])
                {
                    self.write(world, pos, next);
                }
            }
            "note_block" => {
                let powered = input_power(world, pos, &self.analog) > 0;
                if powered && !property_bool(state, "powered") {
                    let note = (property_u8(state, "note").unwrap_or(0) + 1) % 25;
                    let note = NOTE[usize::from(note)];
                    if let Some(next) =
                        with_properties(state, &[("powered", "true"), ("note", note)])
                    {
                        self.write(world, pos, next);
                    }
                } else if !powered
                    && property_bool(state, "powered")
                    && let Some(next) = with_properties(state, &[("powered", "false")])
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
        let powered = input_power(world, pos, &self.analog) > 0;
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

    /// `DiodeBlock.isLocked`: a powered diode on either side points into this one.
    fn update_lock(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3], state: u16) {
        let Some(direction) = facing(state) else {
            return;
        };
        // Comparators have no `locked` property; their frozen output is runtime state.
        if property(state, "locked").is_none() {
            return;
        }
        let locked = is_locked(world, pos, direction);
        if locked != property_bool(state, "locked")
            && let Some(next) =
                with_properties(state, &[("locked", if locked { "true" } else { "false" })])
        {
            self.write(world, pos, next);
        }
    }

    fn update_diode(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        let Some(facing) = facing(state) else {
            return;
        };
        let back = facing.opposite();
        let input = power_from(world, pos, back, &self.analog);
        if is_repeater(state) {
            self.update_lock(world, pos, state);
            if is_locked(world, pos, facing) {
                return;
            }
            let powered = input > 0;
            if powered != property_bool(state, "powered")
                && let Some(next) = with_properties(
                    state,
                    &[("powered", if powered { "true" } else { "false" })],
                )
            {
                self.write(world, pos, next);
            }
            return;
        }
        // `ComparatorBlock`: compare the back with the strongest side input. A powered diode
        // on either side locks the comparator's output until the lock is released.
        if is_locked(world, pos, facing) {
            return;
        }
        let sides = perpendicular(facing)
            .into_iter()
            .map(|side| power_from(world, pos, side, &self.analog))
            .max()
            .unwrap_or(0);
        let output = match property(state, "mode") {
            Some("subtract") => input.saturating_sub(sides),
            _ => {
                if input >= sides {
                    input
                } else {
                    0
                }
            }
        };
        let previous = self.analog.insert(pos, output);
        if previous != Some(output) {
            // The comparator's state only carries `powered`; a change between two non-zero
            // strengths must still refresh the wire it feeds.
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

    fn update_torch(&mut self, world: &mut impl RedstoneWorld, pos: [i32; 3]) {
        let Some(state) = world.block(pos) else {
            return;
        };
        if !is_torch(state) {
            return;
        }
        let attach = match name(state) {
            Some("redstone_wall_torch") => facing(state)
                .map(Direction::opposite)
                .unwrap_or(Direction::Down),
            _ => Direction::Down,
        };
        // A torch must not read its own direct signal through the block it is attached to.
        let mut input = 0;
        let attach_pos = offset(pos, attach);
        if let Some(attach_state) = world.signal_block(attach_pos) {
            input = direct_signal(attach_state, None);
            for direction in Direction::ALL {
                let neighbour = offset(attach_pos, direction);
                if neighbour == pos {
                    continue;
                }
                let Some(other) = world.signal_block(neighbour) else {
                    continue;
                };
                let signal = if is_comparator(other) {
                    self.analog.get(&neighbour).copied().unwrap_or(0)
                } else {
                    direct_signal(other, None)
                };
                input = input.max(signal);
            }
        }
        let lit = property_bool(state, "lit");
        let next = if input > 0 {
            if lit { Some(false) } else { None }
        } else if lit {
            None
        } else {
            // Burnout: too many toggles in the recent window keeps the torch dark.
            let recent = self.toggles.entry(pos).or_default();
            recent.retain(|&time| time + TORCH_TOGGLE_WINDOW > self.clock);
            if recent.len() >= TORCH_MAX_TOGGLES {
                self.schedule(pos, Kind::Torch, TORCH_BURNOUT_TICKS);
                None
            } else {
                Some(true)
            }
        };
        let Some(next) = next else {
            return;
        };
        self.toggles.entry(pos).or_default().push_back(self.clock);
        if let Some(state) = with_properties(state, &[("lit", if next { "true" } else { "false" })])
        {
            self.write(world, pos, state);
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

    /// Entities other than players for this tick's pressure plates.
    pub fn set_occupants(&mut self, occupants: Vec<Occupant>) {
        self.occupants = occupants;
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
        self.scheduled.retain(|(pos, kind), due| {
            if keep(pos[0], pos[2]) {
                out.scheduled.insert((*pos, *kind), *due);
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
        for (key, due) in other.scheduled {
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

/// `DiodeBlock.isLocked`: a powered diode on either side points into `pos`.
fn is_locked(world: &impl RedstoneWorld, pos: [i32; 3], direction: Direction) -> bool {
    perpendicular(direction).into_iter().any(|side| {
        let neighbour = offset(pos, side);
        world.signal_block(neighbour).is_some_and(|other| {
            is_diode(other)
                && facing(other) == Some(side.opposite())
                && property_bool(other, "powered")
        })
    })
}

/// Horizontal directions perpendicular to a horizontal facing (`Direction.Plane.HORIZONTAL`).
fn perpendicular(direction: Direction) -> [Direction; 2] {
    match direction {
        Direction::North | Direction::South => [Direction::East, Direction::West],
        Direction::East | Direction::West => [Direction::North, Direction::South],
        Direction::Up | Direction::Down => [Direction::North, Direction::South],
    }
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
        wakes: Vec<[i32; 3]>,
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

        fn wake_redstone(&mut self, positions: &[[i32; 3]]) {
            self.wakes.extend_from_slice(positions);
        }
    }

    #[test]
    fn an_edge_change_wakes_the_neighbouring_region_once() {
        let mut world = Edge::new(true);
        let mut redstone = Redstone::default();
        redstone.blocks_changed(&mut world, &[[15, 64, 0]]);
        assert_eq!(world.wakes, [[16, 64, 0]]);
        world.wakes.clear();
        // Handling a foreign change must not bounce the wake back to its source.
        redstone.foreign_changed(&mut world, [15, 64, 0]);
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
        redstone.foreign_changed(&mut world, [16, 64, 0]);
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
                &[("facing", "east"), ("delay", "2"), ("powered", "false")],
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
            &[("face", "wall"), ("facing", "east"), ("powered", "false")],
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
        assert!(!property_bool(grid.state([2, 64, 0]), "lit"));
    }

    #[test]
    fn comparator_compares_subtracts_and_reports_analog_strength() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], make("target", &[("power", "10")]));
        grid.add([1, 64, 1], make("target", &[("power", "4")]));
        grid.add(
            [1, 64, 0],
            make(
                "comparator",
                &[
                    ("facing", "east"),
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
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("6"));

        grid.add([1, 64, 1], make("target", &[("power", "15")]));
        let compare = with_properties(grid.state([1, 64, 0]), &[("mode", "compare")]).unwrap();
        grid.add([1, 64, 0], compare);
        redstone.blocks_changed(&mut grid, &[[1, 64, 1], [1, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("0"));
        assert!(!property_bool(grid.state([3, 64, 0]), "lit"));
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
        assert_eq!(property(grid.state([9, 64, 0]), "note"), Some("1"));
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
        let source = lever();
        grid.add([0, 64, 0], source);
        grid.add([1, 64, 0], make("stone", &[]));
        grid.add([1, 65, 0], wire());
        grid.add([2, 65, 0], lamp());
        let mut redstone = Redstone::default();

        grid.add([0, 64, 0], use_block(source).unwrap());
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 1);
        assert_eq!(property(grid.state([1, 65, 0]), "power"), Some("15"));
        assert!(property_bool(grid.state([2, 65, 0]), "lit"));
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
    fn comparator_output_freezes_while_locked_and_resumes_after_unlock() {
        let mut grid = Grid::new();
        grid.add([0, 64, 0], make("target", &[("power", "10")]));
        grid.add(
            [1, 64, 0],
            make(
                "comparator",
                &[
                    ("facing", "east"),
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

        // A powered repeater pointing into the comparator's side locks it.
        grid.add([1, 64, 2], make("target", &[("power", "15")]));
        grid.add(
            [1, 64, 1],
            make(
                "repeater",
                &[("facing", "north"), ("delay", "1"), ("powered", "true")],
            ),
        );
        redstone.blocks_changed(&mut grid, &[[1, 64, 1]]);
        tick(&mut redstone, &mut grid, 2);

        grid.add([0, 64, 0], make("target", &[("power", "4")]));
        redstone.blocks_changed(&mut grid, &[[0, 64, 0]]);
        tick(&mut redstone, &mut grid, 2);
        assert_eq!(property(grid.state([2, 64, 0]), "power"), Some("10"));

        grid.add([1, 64, 1], air());
        redstone.blocks_changed(&mut grid, &[[1, 64, 1]]);
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
                &[("facing", "east"), ("delay", "1"), ("powered", "false")],
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
                &[("facing", "north"), ("delay", "1"), ("powered", "true")],
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
            &[("face", "wall"), ("facing", "east"), ("powered", "false")],
        );
        grid.add([0, 63, 0], source);
        let mut redstone = Redstone::default();

        for _ in 0..4 {
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
}
