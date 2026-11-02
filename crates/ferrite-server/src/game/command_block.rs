//! Command blocks (vanilla `CommandBlock` block, `CommandBlockEntity` and `BaseCommandBlock`).
//!
//! The block entity's NBT is authoritative: `/setblock ... {Command:...}`, `/data merge block`
//! and the `SetCommandBlock` packet all write it, chunk save keeps it, and every tick re-reads
//! it. Vanilla keys are used unchanged (`Command`, `auto`, `conditionMet`, `powered`,
//! `TrackOutput`, `LastOutput`, `UpdateLastExecution`, `LastExecution`, `SuccessCount`); the
//! safety latch adds `FailureCount` and `Disabled`.
//!
//! Execution mirrors vanilla's three modes:
//! - **impulse** runs once on a rising redstone edge (the `powered` nibble is remembered in the
//!   block entity, because 26.3's block state has no `powered` property);
//! - **repeat** runs every game tick while powered, and `auto` bypasses the redstone input;
//! - **chain** runs when the block it faces into executes, in the same tick.
//!
//! `LastExecution` is the vanilla double-execution guard: a block whose `LastExecution` equals
//! the current game time is skipped, and `UpdateLastExecution` decides whether a run writes the
//! new time. It is what stops a chain loop from running a block twice in one tick.
//!
//! Command output never reaches players: with `TrackOutput` it becomes `LastOutput`, otherwise
//! it is dropped. The only safety deviation from vanilla is the repeated-failure latch, because
//! a powered repeating block that fails otherwise retries every tick forever.

use ferrite_data::blocks;
use ferrite_nbt::{Compound, Tag};
use ferrite_world::BlockEntity;
use std::collections::{HashSet, VecDeque};

use super::anvil::AnvilChunkSource;

/// The three command block blocks, in vanilla `CommandBlockEntity.Mode` order.
pub const BLOCKS: [&str; 3] = [
    "minecraft:command_block",
    "minecraft:chain_command_block",
    "minecraft:repeating_command_block",
];

/// Vanilla has no chain limit; a self-loop with `UpdateLastExecution` off would never end, so
/// one tick runs at most this many chain blocks (the 256th is the last one executed).
pub const MAX_CHAIN: usize = 256;

/// Consecutive failures after which a repeating block disables itself (`Disabled:1b`). Editing
/// the block with `SetCommandBlock` clears the latch.
pub const MAX_FAILURES: u32 = 20;

/// Vanilla `CommandBlockEntity.Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    Impulse,
    Chain,
    Repeat,
}

impl Mode {
    /// The mode a block name stands for, if it is a command block.
    pub fn of_block(block: &str) -> Option<Self> {
        match block {
            "minecraft:command_block" => Some(Self::Impulse),
            "minecraft:chain_command_block" => Some(Self::Chain),
            "minecraft:repeating_command_block" => Some(Self::Repeat),
            _ => None,
        }
    }

    /// The block this mode is stored as.
    pub fn block(self) -> &'static str {
        BLOCKS[self.id() as usize]
    }

    /// The protocol id of `ServerboundSetCommandBlockPacket`.
    pub fn id(self) -> i32 {
        match self {
            Self::Impulse => 0,
            Self::Chain => 1,
            Self::Repeat => 2,
        }
    }

    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Impulse),
            1 => Some(Self::Chain),
            2 => Some(Self::Repeat),
            _ => None,
        }
    }
}

/// One command block's block entity, as the tick loop reads and writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub mode: Mode,
    pub command: String,
    pub auto: bool,
    pub condition_met: bool,
    /// The last redstone input seen (`powered` NBT key), used for the impulse rising edge.
    pub powered: bool,
    pub track_output: bool,
    pub update_last_execution: bool,
    pub last_execution: i64,
    pub last_output: Option<Tag>,
    pub success_count: i32,
    /// Consecutive failed runs (non-vanilla, see [`MAX_FAILURES`]).
    pub failure_count: u32,
    /// Non-vanilla latch set after [`MAX_FAILURES`] consecutive failures.
    pub disabled: bool,
}

fn boolean(data: &Compound, key: &str, default: bool) -> bool {
    match data.get(key) {
        Some(Tag::Byte(value)) => *value != 0,
        Some(Tag::Short(value)) => *value != 0,
        Some(Tag::Int(value)) => *value != 0,
        Some(Tag::Long(value)) => *value != 0,
        _ => default,
    }
}

impl State {
    /// A fresh block entity: empty command, output tracking on (vanilla defaults).
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            command: String::new(),
            auto: false,
            condition_met: false,
            powered: false,
            track_output: true,
            update_last_execution: true,
            last_execution: 0,
            last_output: None,
            success_count: 0,
            failure_count: 0,
            disabled: false,
        }
    }

    /// Vanilla `BaseCommandBlock.load`, plus the safety latch.
    pub fn load(mode: Mode, data: &Compound) -> Self {
        let mut state = Self::new(mode);
        if let Some(Tag::String(command)) = data.get("Command") {
            state.command.clone_from(command);
        }
        state.auto = boolean(data, "auto", false);
        state.condition_met = boolean(data, "conditionMet", false);
        state.powered = boolean(data, "powered", false);
        state.track_output = boolean(data, "TrackOutput", true);
        state.update_last_execution = boolean(data, "UpdateLastExecution", true);
        state.last_execution = match data.get("LastExecution") {
            Some(Tag::Long(value)) => *value,
            Some(Tag::Int(value)) => i64::from(*value),
            _ => 0,
        };
        state.last_output = data.get("LastOutput").cloned();
        state.success_count = match data.get("SuccessCount") {
            Some(Tag::Int(value)) => *value,
            _ => 0,
        };
        state.failure_count = match data.get("FailureCount") {
            Some(Tag::Int(value)) => u32::try_from(*value).unwrap_or(0),
            _ => 0,
        };
        state.disabled = boolean(data, "Disabled", false);
        state
    }

    /// Vanilla `BaseCommandBlock.save`; keys with default values are written too, so a block
    /// edited through `/data` keeps the same shape as one saved by vanilla.
    pub fn save(&self, data: &mut Compound) {
        data.insert("Command".into(), Tag::String(self.command.clone()));
        data.insert("auto".into(), Tag::Byte(self.auto as i8));
        data.insert("conditionMet".into(), Tag::Byte(self.condition_met as i8));
        data.insert("powered".into(), Tag::Byte(self.powered as i8));
        data.insert("TrackOutput".into(), Tag::Byte(self.track_output as i8));
        data.insert(
            "UpdateLastExecution".into(),
            Tag::Byte(self.update_last_execution as i8),
        );
        data.insert("LastExecution".into(), Tag::Long(self.last_execution));
        data.insert("SuccessCount".into(), Tag::Int(self.success_count));
        match &self.last_output {
            Some(output) => {
                data.insert("LastOutput".into(), output.clone());
            }
            None => {
                data.shift_remove("LastOutput");
            }
        }
        if self.failure_count > 0 {
            data.insert(
                "FailureCount".into(),
                Tag::Int(i32::try_from(self.failure_count).unwrap_or(i32::MAX)),
            );
        } else {
            data.shift_remove("FailureCount");
        }
        if self.disabled {
            data.insert("Disabled".into(), Tag::Byte(1));
        } else {
            data.shift_remove("Disabled");
        }
    }
}

fn block_at(
    source: &AnvilChunkSource,
    [x, y, z]: [i32; 3],
) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    let state = u16::try_from(source.block(x, y, z)?).ok()?;
    blocks::state(state)
}

fn property<'a>(props: &'a [(&'static str, &'static str)], key: &str) -> Option<&'a str> {
    props
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// The block a command block's `facing` points at: the next chain block.
fn front([x, y, z]: [i32; 3], props: &[(&'static str, &'static str)]) -> Option<[i32; 3]> {
    let step = match property(props, "facing")? {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    };
    Some([x + step[0], y + step[1], z + step[2]])
}

/// Whether the block state's `conditional` property is set (chain blocks only).
fn conditional(props: &[(&'static str, &'static str)]) -> bool {
    property(props, "conditional") == Some("true")
}

/// All three command block blocks share one block entity type in the registry, and the save
/// format writes its name (`minecraft:command_block`) as `id`, not the block's own name.
const ENTITY_TYPE: &str = "minecraft:command_block";

fn type_id() -> Option<u32> {
    u32::try_from(ferrite_data::registries::id(
        "minecraft:block_entity_type",
        ENTITY_TYPE,
    )?)
    .ok()
}

fn entity_for(_block: &str, [x, y, z]: [i32; 3]) -> Option<BlockEntity> {
    let mut data = Compound::new();
    data.insert("id".into(), Tag::String(ENTITY_TYPE.into()));
    data.insert("x".into(), Tag::Int(x));
    data.insert("y".into(), Tag::Int(y));
    data.insert("z".into(), Tag::Int(z));
    Some(BlockEntity {
        type_id: type_id()?,
        data,
    })
}

/// The block entity at `pos`, created with vanilla defaults when the block is a command block
/// that does not have one yet (vanilla creates it when the block is placed).
pub fn ensure(source: &AnvilChunkSource, pos: [i32; 3], block: &str) -> Option<State> {
    let mode = Mode::of_block(block)?;
    if let Some(entity) = source.block_entity(pos[0], pos[1], pos[2]) {
        return Some(State::load(mode, &entity.data));
    }
    let fresh = entity_for(block, pos)?;
    let state = State::load(mode, &fresh.data);
    source
        .update_block_entity(pos[0], pos[1], pos[2], |entity| {
            if entity.is_some() {
                return (false, ());
            }
            *entity = Some(fresh.clone());
            (true, ())
        })
        .ok()?;
    Some(state)
}

/// Writes a command block state back, keeping every NBT key this module does not own.
pub fn store(source: &AnvilChunkSource, pos: [i32; 3], block: &str, state: &State) -> bool {
    let Some(fresh) = entity_for(block, pos) else {
        return false;
    };
    source
        .update_block_entity(pos[0], pos[1], pos[2], |entity| {
            let entity = entity.get_or_insert(fresh.clone());
            state.save(&mut entity.data);
            (true, ())
        })
        .is_ok()
}

/// Creates (or replaces) the block entity at `pos` and writes `state`. Used by `/setblock`.
pub fn place(source: &AnvilChunkSource, pos: [i32; 3], block: &str, state: &State) -> bool {
    let Some(fresh) = entity_for(block, pos) else {
        return false;
    };
    source
        .update_block_entity(pos[0], pos[1], pos[2], |entity| {
            let entity = entity.get_or_insert(fresh.clone());
            state.save(&mut entity.data);
            (true, ())
        })
        .is_ok()
}

/// `/setblock ... {nbt}`: creates the block entity with the given tag, then fills in the keys
/// the command block owns (leaving every other key of the tag intact).
pub fn place_with(source: &AnvilChunkSource, pos: [i32; 3], block: &str, data: &Compound) -> bool {
    let Some(mode) = Mode::of_block(block) else {
        return false;
    };
    let Some(fresh) = entity_for(block, pos) else {
        return false;
    };
    let mut entity = fresh;
    for (key, value) in data {
        entity.data.insert(key.clone(), value.clone());
    }
    let state = State::load(mode, &entity.data);
    state.save(&mut entity.data);
    source
        .update_block_entity(pos[0], pos[1], pos[2], |slot| {
            *slot = Some(entity.clone());
            (true, ())
        })
        .is_ok()
}

/// Applies a `SetCommandBlock` editor packet (vanilla `CommandBlockEntity` setters). The mode
/// is stored in the block, the conditional flag in the block state, the rest in the NBT.
pub fn edit(
    source: &AnvilChunkSource,
    pos: [i32; 3],
    packet: &ferrite_protocol::packets::play::SetCommandBlock,
) -> bool {
    let [x, y, z] = pos;
    let Some((block, props)) = block_at(source, pos) else {
        return false;
    };
    let Some(mode) = Mode::of_block(block) else {
        return false;
    };
    let Some(new_mode) = Mode::from_id(packet.mode.0) else {
        return false;
    };
    let facing = property(props, "facing").unwrap_or("north");
    let conditional_value = if packet.conditional { "true" } else { "false" };
    if new_mode != mode || conditional(props) != packet.conditional {
        let Some(state) = blocks::state_id(
            new_mode.block(),
            &[("facing", facing), ("conditional", conditional_value)],
        ) else {
            return false;
        };
        if source.set_block(x, y, z, u32::from(state)).is_err() {
            return false;
        }
    }
    let mut state = source.block_entity(x, y, z).map_or_else(
        || State::new(new_mode),
        |entity| State::load(new_mode, &entity.data),
    );
    state.mode = new_mode;
    state.command = packet.command.clone();
    state.track_output = packet.track_output;
    state.auto = packet.automatic;
    // Editing a block clears the safety latch, so a fixed command can run again.
    state.disabled = false;
    state.failure_count = 0;
    place(source, pos, new_mode.block(), &state)
}

/// The command source a command block runs with (implemented by the region).
pub trait Runner {
    /// Runs `command` as the block at `pos` (permission 2, the block position as context).
    /// `Err` carries the failure message that becomes `LastOutput`.
    fn run(&mut self, pos: [i32; 3], command: &str) -> Result<String, String>;
}

/// The region's command blocks: positions seen so far and this tick's chain queue.
#[derive(Debug, Default)]
pub struct CommandBlocks {
    tracked: HashSet<[i32; 3]>,
    pending: VecDeque<([i32; 3], bool)>,
}

impl CommandBlocks {
    pub fn track(&mut self, pos: [i32; 3]) {
        self.tracked.insert(pos);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.tracked.len()
    }

    /// Moves the tracked positions of `sections` into a new region (`RegionData::split`).
    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut result = Self::default();
        let moving: Vec<[i32; 3]> = self
            .tracked
            .iter()
            .copied()
            .filter(|pos| keep(pos[0], pos[2]))
            .collect();
        for pos in moving {
            self.tracked.remove(&pos);
            result.tracked.insert(pos);
        }
        result
    }

    /// Absorbs another region's tracked blocks (`RegionData::merge`).
    pub fn merge(&mut self, other: Self) {
        self.tracked.extend(other.tracked);
    }

    /// Picks up command blocks whose neighbours changed (a lever next to an impulse block, a
    /// `/setblock` or a placed block) and the changed block itself.
    pub fn blocks_changed(&mut self, source: &AnvilChunkSource, changed: &[[i32; 3]]) {
        for &pos in changed {
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let target = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                if block_at(source, target).is_some_and(|(name, _)| Mode::of_block(name).is_some())
                {
                    self.track(target);
                }
            }
        }
    }

    /// Runs one region tick's command blocks. `power` is the redstone input at a block and
    /// `game_time` the world tick, which `LastExecution` is compared against.
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        power: &dyn Fn([i32; 3]) -> u8,
        runner: &mut dyn Runner,
        game_time: i64,
    ) -> Vec<([i32; 3], BlockEntity)> {
        let mut updated = Vec::new();
        let mut positions: Vec<[i32; 3]> = self.tracked.iter().copied().collect();
        positions.sort_unstable();
        self.pending.clear();
        for pos in positions {
            let Some((block, props)) = block_at(source, pos) else {
                self.tracked.remove(&pos);
                continue;
            };
            let Some(mode) = Mode::of_block(block) else {
                self.tracked.remove(&pos);
                continue;
            };
            let Some(before) = ensure(source, pos, block) else {
                continue;
            };
            let mut state = before.clone();
            match mode {
                Mode::Repeat => {
                    let powered = power(pos) != 0;
                    state.powered = powered;
                    if !state.disabled && (powered || state.auto) {
                        self.execute(source, runner, game_time, pos, props, &mut state);
                    }
                }
                Mode::Impulse => {
                    let powered = power(pos) != 0;
                    let rising = powered && !state.powered;
                    state.powered = powered;
                    if rising && !state.disabled {
                        self.execute(source, runner, game_time, pos, props, &mut state);
                    }
                }
                // A chain block only runs from its predecessor (`self.pending`).
                Mode::Chain => {}
            }
            if state != before
                && store(source, pos, block, &state)
                && let Some(entity) = source.block_entity(pos[0], pos[1], pos[2])
            {
                updated.push((pos, entity));
            }
        }
        self.run_chain(source, runner, game_time, &mut updated);
        updated
    }

    /// Runs the chain blocks woken by a predecessor, in trigger order, up to [`MAX_CHAIN`].
    fn run_chain(
        &mut self,
        source: &AnvilChunkSource,
        runner: &mut dyn Runner,
        game_time: i64,
        updated: &mut Vec<([i32; 3], BlockEntity)>,
    ) {
        let mut budget = MAX_CHAIN;
        while let Some((pos, predecessor_ok)) = self.pending.pop_front() {
            if budget == 0 {
                tracing::warn!(
                    limit = MAX_CHAIN,
                    "command block chain cap reached; dropping the rest of this tick"
                );
                self.pending.clear();
                break;
            }
            budget -= 1;
            let Some((block, props)) = block_at(source, pos) else {
                continue;
            };
            if Mode::of_block(block) != Some(Mode::Chain) {
                continue;
            }
            let Some(before) = ensure(source, pos, block) else {
                continue;
            };
            let mut state = before.clone();
            state.condition_met = predecessor_ok;
            if !state.disabled && (!conditional(props) || predecessor_ok) {
                self.execute(source, runner, game_time, pos, props, &mut state);
            }
            if state != before
                && store(source, pos, block, &state)
                && let Some(entity) = source.block_entity(pos[0], pos[1], pos[2])
            {
                updated.push((pos, entity));
            }
        }
    }

    /// Runs one command and applies the vanilla bookkeeping: the `LastExecution` guard, the
    /// output, the failure latch and waking the chain block in front.
    fn execute(
        &mut self,
        source: &AnvilChunkSource,
        runner: &mut dyn Runner,
        game_time: i64,
        pos: [i32; 3],
        props: &[(&'static str, &'static str)],
        state: &mut State,
    ) {
        if state.command.trim().is_empty() || state.last_execution == game_time {
            return;
        }
        let (success, output) = match runner.run(pos, &state.command) {
            Ok(output) => (true, output),
            Err(error) => (false, error),
        };
        if state.track_output {
            state.last_output = Some(Tag::String(output));
        }
        state.success_count = i32::from(success);
        if state.update_last_execution {
            state.last_execution = game_time;
        }
        if success {
            state.failure_count = 0;
        } else if state.mode == Mode::Repeat {
            state.failure_count += 1;
            if state.failure_count >= MAX_FAILURES {
                state.disabled = true;
                tracing::warn!(
                    ?pos,
                    command = %state.command,
                    limit = MAX_FAILURES,
                    "repeating command block disabled after repeated failures"
                );
            }
        }
        if let Some(next) = front(pos, props)
            && block_at(source, next)
                .is_some_and(|(name, _)| Mode::of_block(name) == Some(Mode::Chain))
        {
            self.pending.push_back((next, success));
        }
    }
}

/// Vanilla `PistonBaseBlock.isPushable`: command blocks are not movable. Pistons are not in
/// this tree yet (another worker owns them); this is the check their push table must call.
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_unmovable(state: u16) -> bool {
    blocks::state(state).is_some_and(|(name, _)| Mode::of_block(name).is_some())
}

#[cfg(test)]
mod tests {
    use super::super::commands_data::{BlockData, apply_block};
    use super::*;
    use ferrite_nbt::snbt;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Default)]
    struct Recorder {
        fail: bool,
        runs: Vec<(String, [i32; 3])>,
    }

    impl Runner for Recorder {
        fn run(&mut self, pos: [i32; 3], command: &str) -> Result<String, String> {
            self.runs.push((command.to_owned(), pos));
            if self.fail || command.starts_with("fail") {
                Err(format!("unknown command '{command}'"))
            } else {
                Ok(format!("ran '{command}'"))
            }
        }
    }

    async fn source() -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        load(&source, 0, 0).await;
        source
    }

    /// Generates one chunk of the flat test world, so blocks in it can be edited.
    async fn load(source: &Arc<AnvilChunkSource>, x: i32, z: i32) {
        while source.get(x, z, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    fn state_of(block: &str, facing: &str, conditional: bool) -> u16 {
        ferrite_data::blocks::state_id(
            block,
            &[
                ("facing", facing),
                ("conditional", if conditional { "true" } else { "false" }),
            ],
        )
        .expect("command block state")
    }

    fn place(source: &AnvilChunkSource, pos: [i32; 3], block: &str, facing: &str) -> State {
        source
            .set_block(
                pos[0],
                pos[1],
                pos[2],
                u32::from(state_of(block, facing, false)),
            )
            .unwrap();
        ensure(source, pos, block).expect("block entity")
    }

    fn write(source: &AnvilChunkSource, pos: [i32; 3], block: &str, f: impl FnOnce(&mut State)) {
        let mut state = ensure(source, pos, block).expect("block entity");
        f(&mut state);
        assert!(store(source, pos, block, &state));
    }

    fn power_grid(power: &HashMap<[i32; 3], u8>) -> impl Fn([i32; 3]) -> u8 + '_ {
        move |pos| power.get(&pos).copied().unwrap_or(0)
    }

    #[test]
    fn modes_map_to_their_blocks_and_protocol_ids() {
        for (id, block) in BLOCKS.iter().enumerate() {
            let mode = Mode::of_block(block).unwrap();
            assert_eq!(mode.block(), *block);
            assert_eq!(mode.id(), id as i32);
            assert_eq!(Mode::from_id(id as i32), Some(mode));
        }
        assert_eq!(Mode::of_block("minecraft:stone"), None);
        assert_eq!(Mode::from_id(3), None);
    }

    #[test]
    fn state_round_trips_every_vanilla_key() {
        let mut state = State::new(Mode::Chain);
        state.command = "say hi".into();
        state.auto = true;
        state.condition_met = true;
        state.powered = true;
        state.track_output = false;
        state.update_last_execution = false;
        state.last_execution = 41;
        state.last_output = Some(Tag::String("{\"text\":\"hi\"}".into()));
        state.success_count = 1;
        let mut data = Compound::new();
        data.insert("Custom".into(), Tag::Int(5));
        state.save(&mut data);
        assert_eq!(State::load(Mode::Chain, &data), state);
        // Keys this module does not own survive.
        assert_eq!(data.get("Custom"), Some(&Tag::Int(5)));
        assert_eq!(state.mode.block(), "minecraft:chain_command_block");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nbt_round_trips_through_data_merge_and_get() {
        let source = source().await;
        let pos = [0, 70, 0];
        place(&source, pos, BLOCKS[0], "east");
        assert_eq!(source.block_entity(0, 70, 0).unwrap().type_id, 23);
        // `/data merge block 0 70 0 {Command:"say hi",auto:1b,TrackOutput:1b}`
        let data = snbt::parse("{Command:\"say hi\",auto:1b,TrackOutput:1b}").unwrap();
        apply_block(
            Some(&source),
            &BlockData::Merge {
                pos,
                data: data.clone(),
            },
        )
        .unwrap();
        let entity = source.block_entity(0, 70, 0).unwrap();
        let state = State::load(Mode::Impulse, &entity.data);
        assert_eq!(state.command, "say hi");
        assert!(state.auto);
        assert!(state.track_output);
        // `/data get block 0 70 0 Command` sees the merged value.
        let feedback = apply_block(
            Some(&source),
            &BlockData::Get {
                pos,
                path: "Command".into(),
            },
        )
        .unwrap();
        assert!(feedback.text.contains("say hi"), "{}", feedback.text);
        // A state rewrite keeps the rest of the NBT (for example a custom key).
        let mut state = state;
        state.last_execution = 9;
        source
            .update_block_entity(0, 70, 0, |entity| {
                entity
                    .as_mut()
                    .unwrap()
                    .data
                    .insert("Custom".into(), Tag::Int(7));
                (true, ())
            })
            .unwrap();
        assert!(store(&source, pos, BLOCKS[0], &state));
        let entity = source.block_entity(0, 70, 0).unwrap();
        assert_eq!(entity.data.get("Custom"), Some(&Tag::Int(7)));
        assert_eq!(State::load(Mode::Impulse, &entity.data), state);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn setblock_nbt_creates_the_block_entity() {
        let source = source().await;
        let pos = [7, 70, 0];
        source
            .set_block(7, 70, 0, u32::from(state_of(BLOCKS[0], "east", false)))
            .unwrap();
        let Tag::Compound(data) = snbt::parse("{Command:\"say hi\",auto:1b,Custom:3}").unwrap()
        else {
            panic!("compound tag");
        };
        assert!(place_with(&source, pos, BLOCKS[0], &data));
        let entity = source.block_entity(7, 70, 0).unwrap();
        let state = State::load(Mode::Impulse, &entity.data);
        assert_eq!(state.command, "say hi");
        assert!(state.auto);
        // Keys that are not command block fields are kept by the new block entity.
        assert_eq!(entity.data.get("Custom"), Some(&Tag::Int(3)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn impulse_runs_once_per_rising_edge() {
        let source = source().await;
        let pos = [0, 70, 0];
        place(&source, pos, BLOCKS[0], "east");
        write(&source, pos, BLOCKS[0], |state| {
            state.command = "say hi".into()
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(pos);
        let mut power = HashMap::new();
        let mut runner = Recorder::default();
        blocks.tick(&source, &power_grid(&power), &mut runner, 1);
        assert!(
            runner.runs.is_empty(),
            "unpowered impulse block must not run"
        );
        power.insert(pos, 15);
        blocks.tick(&source, &power_grid(&power), &mut runner, 2);
        assert_eq!(runner.runs.len(), 1);
        // Still powered: no second run.
        blocks.tick(&source, &power_grid(&power), &mut runner, 3);
        blocks.tick(&source, &power_grid(&power), &mut runner, 4);
        assert_eq!(runner.runs.len(), 1);
        // Falling edge, then a new rising edge runs again.
        power.remove(&pos);
        blocks.tick(&source, &power_grid(&power), &mut runner, 5);
        power.insert(pos, 15);
        blocks.tick(&source, &power_grid(&power), &mut runner, 6);
        assert_eq!(runner.runs.len(), 2);
        assert_eq!(runner.runs[1].1, pos);
        let entity = source.block_entity(0, 70, 0).unwrap();
        let state = State::load(Mode::Impulse, &entity.data);
        assert!(state.powered);
        assert_eq!(state.success_count, 1);
        assert_eq!(
            state.last_output,
            Some(Tag::String("ran 'say hi'".into())),
            "TrackOutput defaults to on"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn repeat_runs_every_tick_while_powered_and_auto_bypasses_redstone() {
        let source = source().await;
        let pos = [2, 70, 0];
        place(&source, pos, BLOCKS[2], "south");
        write(&source, pos, BLOCKS[2], |state| {
            state.command = "say tick".into();
            // Runs every tick with the same game time; each tick gets a new one below.
            state.update_last_execution = false;
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(pos);
        let mut power = HashMap::new();
        let mut runner = Recorder::default();
        for tick in 1..=3 {
            blocks.tick(&source, &power_grid(&power), &mut runner, tick);
        }
        assert!(runner.runs.is_empty(), "unpowered and not automatic");
        power.insert(pos, 15);
        for tick in 4..=6 {
            blocks.tick(&source, &power_grid(&power), &mut runner, tick);
        }
        assert_eq!(runner.runs.len(), 3, "one run per tick while powered");
        // `auto` runs without redstone.
        power.remove(&pos);
        write(&source, pos, BLOCKS[2], |state| state.auto = true);
        for tick in 7..=9 {
            blocks.tick(&source, &power_grid(&power), &mut runner, tick);
        }
        assert_eq!(runner.runs.len(), 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chain_follows_its_predecessor_and_conditional_blocks_skip_failures() {
        let source = source().await;
        let impulse = [0, 70, 0];
        let chain = [1, 70, 0];
        place(&source, impulse, BLOCKS[0], "east");
        place(&source, chain, BLOCKS[1], "east");
        write(&source, impulse, BLOCKS[0], |state| {
            state.command = "say a".into()
        });
        write(&source, chain, BLOCKS[1], |state| {
            state.command = "say b".into()
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(impulse);
        blocks.track(chain);
        let mut power = HashMap::new();
        let mut runner = Recorder::default();
        power.insert(impulse, 15);
        blocks.tick(&source, &power_grid(&power), &mut runner, 1);
        assert_eq!(
            runner
                .runs
                .iter()
                .map(|(command, _)| command.as_str())
                .collect::<Vec<_>>(),
            vec!["say a", "say b"],
            "the chain block runs in the same tick as its predecessor"
        );
        let entity = source.block_entity(1, 70, 0).unwrap();
        assert!(State::load(Mode::Chain, &entity.data).condition_met);

        // A failing predecessor skips a conditional chain block and records conditionMet=false.
        let source2 = source.clone();
        let far = [5, 70, 0];
        let chain2 = [6, 70, 0];
        place(&source2, far, BLOCKS[0], "east");
        source2
            .set_block(6, 70, 0, u32::from(state_of(BLOCKS[1], "east", true)))
            .unwrap();
        write(&source2, far, BLOCKS[0], |state| {
            state.command = "fail here".into()
        });
        ensure(&source2, chain2, BLOCKS[1]);
        write(&source2, chain2, BLOCKS[1], |state| {
            state.command = "say never".into()
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(far);
        blocks.track(chain2);
        let mut power = HashMap::new();
        power.insert(far, 15);
        let mut runner = Recorder::default();
        blocks.tick(&source2, &power_grid(&power), &mut runner, 2);
        assert_eq!(
            runner
                .runs
                .iter()
                .map(|(command, _)| command.as_str())
                .collect::<Vec<_>>(),
            vec!["fail here"]
        );
        let entity = source2.block_entity(6, 70, 0).unwrap();
        let state = State::load(Mode::Chain, &entity.data);
        assert!(!state.condition_met);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn last_execution_guards_a_second_run_in_the_same_tick() {
        let source = source().await;
        let pos = [3, 70, 0];
        place(&source, pos, BLOCKS[2], "east");
        write(&source, pos, BLOCKS[2], |state| {
            state.command = "say once".into();
            state.auto = true;
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(pos);
        let mut runner = Recorder::default();
        let power = HashMap::new();
        // Two ticks with the same game time: `UpdateLastExecution` (default on) writes the
        // time on the first run, and the guard skips the second.
        blocks.tick(&source, &power_grid(&power), &mut runner, 10);
        blocks.tick(&source, &power_grid(&power), &mut runner, 10);
        assert_eq!(runner.runs.len(), 1);
        let entity = source.block_entity(3, 70, 0).unwrap();
        assert_eq!(State::load(Mode::Repeat, &entity.data).last_execution, 10);
        // Turning `UpdateLastExecution` off (with the stale time cleared) removes the guard:
        // this is the documented reason a chain loop needs the explicit cap.
        write(&source, pos, BLOCKS[2], |state| {
            state.update_last_execution = false;
            state.last_execution = 0;
        });
        blocks.tick(&source, &power_grid(&power), &mut runner, 10);
        assert_eq!(runner.runs.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_chain_loop_stops_at_the_documented_cap() {
        let source = source().await;
        load(&source, 1, 0).await;
        let first = [20, 70, 0];
        let second = [21, 70, 0];
        source
            .set_block(20, 70, 0, u32::from(state_of(BLOCKS[1], "east", false)))
            .unwrap();
        source
            .set_block(21, 70, 0, u32::from(state_of(BLOCKS[1], "west", false)))
            .unwrap();
        ensure(&source, first, BLOCKS[1]);
        ensure(&source, second, BLOCKS[1]);
        for pos in [first, second] {
            write(&source, pos, BLOCKS[1], |state| {
                state.command = "say loop".into();
                state.update_last_execution = false;
            });
        }
        let mut blocks = CommandBlocks::default();
        blocks.track(first);
        blocks.track(second);
        // Seed the loop through the public queue by running a predecessor chain trigger.
        blocks.pending.push_back((first, true));
        let mut runner = Recorder::default();
        blocks.run_chain(&source, &mut runner, 30, &mut Vec::new());
        assert_eq!(
            runner.runs.len(),
            MAX_CHAIN,
            "the cap bounds one tick's chain"
        );
        assert!(blocks.pending.is_empty(), "the rest of the loop is dropped");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_repeating_block_that_keeps_failing_disables_itself() {
        let source = source().await;
        load(&source, 1, 0).await;
        let pos = [30, 70, 0];
        place(&source, pos, BLOCKS[2], "east");
        write(&source, pos, BLOCKS[2], |state| {
            state.command = "fail always".into();
            state.auto = true;
        });
        let mut blocks = CommandBlocks::default();
        blocks.track(pos);
        let mut runner = Recorder::default();
        let power = HashMap::new();
        for tick in 1..=MAX_FAILURES as i64 + 5 {
            blocks.tick(&source, &power_grid(&power), &mut runner, tick);
        }
        assert_eq!(
            runner.runs.len(),
            MAX_FAILURES as usize,
            "no further attempt after the latch"
        );
        let entity = source.block_entity(30, 70, 0).unwrap();
        let state = State::load(Mode::Repeat, &entity.data);
        assert!(state.disabled);
        assert_eq!(state.failure_count, MAX_FAILURES);
        assert_eq!(
            state.last_output,
            Some(Tag::String("unknown command 'fail always'".into()))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocks_changed_discovers_a_command_block_next_to_a_lever() {
        let source = source().await;
        load(&source, 2, 0).await;
        source
            .set_block(40, 70, 0, u32::from(state_of(BLOCKS[0], "east", false)))
            .unwrap();
        let mut blocks = CommandBlocks::default();
        // A lever at 40 70 -1 turns on; its neighbour set includes the command block.
        blocks.blocks_changed(&source, &[[40, 70, -1]]);
        assert_eq!(blocks.len(), 1);
        // The command block itself changing is discovered too.
        source
            .set_block(45, 70, 0, u32::from(state_of(BLOCKS[2], "east", false)))
            .unwrap();
        blocks.blocks_changed(&source, &[[45, 70, 0]]);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn command_blocks_are_unmovable_by_pistons() {
        for block in BLOCKS {
            let state = state_of(block, "north", false);
            assert!(is_unmovable(state), "{block} must not be pushed");
        }
        assert!(!is_unmovable(
            ferrite_data::blocks::default_state("minecraft:stone").unwrap()
        ));
    }
}
