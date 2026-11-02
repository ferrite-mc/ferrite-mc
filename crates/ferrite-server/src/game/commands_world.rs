//! World-editing commands: `/setblock`, `/fill` and `/clone`.
//!
//! Parsing and validation happen here, on the command thread; the block writes run on the
//! region thread through the same [`Player::change_block`] path players use, so every client
//! tracking the chunk receives the ordinary `BlockUpdate`. Vanilla translation keys for the
//! messages are noted next to each string (the strings themselves are the 26.3 `en_us` text).

use super::commands::{Action, Source};
use super::region::{Player, Tracking};
use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, argument,
    arguments::{BlockStateValue, Coordinates},
    literal,
};
use ferrite_nbt::Tag;
use ferrite_net::game::packet;
use ferrite_protocol::{
    VarInt,
    packets::play::{BlockUpdate, SystemChat},
    types::{BlockPos, PlainText},
};
use ferrite_region::ChunkPos;

/// Vanilla's cap on the volume of a single `/fill` or `/clone`.
pub const MAX_BLOCKS: u128 = 32768;

/// How `/setblock` treats the block that is already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetMode {
    Replace,
    Keep,
    Destroy,
}

/// How `/fill` treats the blocks inside the cuboid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    Replace,
    Keep,
    Destroy,
    Hollow,
    Outline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneMode {
    Replace,
    Masked,
}

/// A parsed block-state argument. Unspecified properties are wildcards, like vanilla's block
/// predicate in `fill ... replace <filter>`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockFilter {
    id: String,
    props: Vec<(String, String)>,
}

impl BlockFilter {
    fn matches(&self, state: u32) -> bool {
        let Ok(state) = u16::try_from(state) else {
            return false;
        };
        let Some((name, props)) = ferrite_data::blocks::state(state) else {
            return false;
        };
        normalized(name) == self.id
            && self
                .props
                .iter()
                .all(|(key, value)| props.iter().any(|(k, v)| k == key && v == value))
    }
}

/// The `/setblock`, `/fill` and `/clone` tree's runtime actions.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldCommand {
    SetBlock {
        pos: Coordinates,
        block: u16,
        mode: SetMode,
        /// The `{...}` block entity tag of `/setblock ... {Command:...}`.
        nbt: Option<Tag>,
    },
    Fill {
        from: Coordinates,
        to: Coordinates,
        block: u16,
        mode: FillMode,
        filter: Option<BlockFilter>,
    },
    Clone {
        begin: Coordinates,
        end: Coordinates,
        destination: Coordinates,
        mode: CloneMode,
    },
}

/// A source position and rotation, used to resolve `~` and `^` coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Origin {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// One changed block, in the order it should be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub location: i64,
    pub state: u32,
}

/// The writes a command performed and the feedback line to send.
#[derive(Debug, Default)]
pub struct Outcome {
    pub changes: Vec<Change>,
    pub message: String,
}

/// The world state a command needs. Implemented for the chunk source and by tests.
pub trait WorldBlocks {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u32>;
    fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool;
    /// Creates the block entity a freshly placed block needs. Only command blocks are
    /// supported; `nbt` is the `/setblock` tag, which wins over the defaults.
    fn place_block_entity(&self, _pos: [i32; 3], _state: u32, _nbt: Option<&Tag>) -> bool {
        false
    }
}

impl WorldBlocks for super::anvil::AnvilChunkSource {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        super::anvil::AnvilChunkSource::block(self, x, y, z)
    }
    fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool {
        super::anvil::AnvilChunkSource::set_block(self, x, y, z, state).is_ok()
    }
    fn place_block_entity(&self, pos: [i32; 3], state: u32, nbt: Option<&Tag>) -> bool {
        let Some((name, _)) = u16::try_from(state)
            .ok()
            .and_then(ferrite_data::blocks::state)
        else {
            return false;
        };
        if super::command_block::Mode::of_block(name).is_none() {
            return false;
        }
        match nbt {
            Some(Tag::Compound(data)) => super::command_block::place_with(self, pos, name, data),
            _ => super::command_block::ensure(self, pos, name).is_some(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tree registration.
// ---------------------------------------------------------------------------------------------

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(setblock_tree());
    tree.register(fill_tree());
    tree.register(clone_tree());
}

fn dispatch(source: &Source, command: WorldCommand) -> Result<i32, CommandError> {
    source
        .actions
        .send(Action::World(command))
        .map_err(|_| CommandError::failed("command target unavailable"))?;
    Ok(1)
}

fn position(ctx: &CommandContext<Source>, name: &str) -> Result<Coordinates, CommandError> {
    match ctx.argument(name) {
        Some(ferrite_command::ArgumentValue::BlockPos(coords)) => Ok(*coords),
        _ => Err(CommandError::failed(format!("missing {name}"))),
    }
}

fn block_argument(ctx: &CommandContext<Source>, name: &str) -> Result<u16, CommandError> {
    match ctx.argument(name) {
        Some(ferrite_command::ArgumentValue::BlockState(value)) => resolve_block(value),
        _ => Err(CommandError::failed(format!("missing {name}"))),
    }
}

/// The `{...}` block entity tag of a block argument (`/setblock ... {Command:...}`).
fn nbt_argument(ctx: &CommandContext<Source>) -> Result<Option<Tag>, CommandError> {
    match ctx.argument("block") {
        Some(ferrite_command::ArgumentValue::BlockState(value)) => match &value.nbt {
            Some(text) => ferrite_nbt::snbt::parse(text)
                .map(Some)
                .map_err(|error| CommandError::failed(error.to_string())),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

fn filter_argument(ctx: &CommandContext<Source>) -> Result<BlockFilter, CommandError> {
    match ctx.argument("filter") {
        Some(ferrite_command::ArgumentValue::BlockState(value)) => resolve_filter(value),
        _ => Err(CommandError::failed("missing filter")),
    }
}

fn setblock_tree() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("setblock")
        .requires(|s| s.permission >= 2)
        .then(
            argument("pos", ArgumentType::BlockPos).then(
                argument("block", ArgumentType::BlockState)
                    .executes(setblock_execute(SetMode::Replace))
                    .then(literal("replace").executes(setblock_execute(SetMode::Replace)))
                    .then(literal("destroy").executes(setblock_execute(SetMode::Destroy)))
                    .then(literal("keep").executes(setblock_execute(SetMode::Keep))),
            ),
        )
}

fn setblock_execute(
    mode: SetMode,
) -> impl Fn(&CommandContext<Source>) -> Result<i32, CommandError> + Send + Sync + 'static {
    move |ctx| {
        let pos = position(ctx, "pos")?;
        let block = block_argument(ctx, "block")?;
        let nbt = nbt_argument(ctx)?;
        dispatch(
            ctx.source(),
            WorldCommand::SetBlock {
                pos,
                block,
                mode,
                nbt,
            },
        )
    }
}

fn fill_tree() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("fill")
        .requires(|s| s.permission >= 2)
        .then(
            argument("from", ArgumentType::BlockPos).then(
                argument("to", ArgumentType::BlockPos).then(
                    argument("block", ArgumentType::BlockState)
                        .executes(fill_execute(FillMode::Replace, false))
                        .then(fill_mode("replace", FillMode::Replace))
                        .then(fill_mode("destroy", FillMode::Destroy))
                        .then(fill_mode("keep", FillMode::Keep))
                        .then(fill_mode("hollow", FillMode::Hollow))
                        .then(fill_mode("outline", FillMode::Outline)),
                ),
            ),
        )
}

/// One `<mode> [filter]` branch of `/fill`.
fn fill_mode(name: &'static str, mode: FillMode) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>(name)
        .executes(fill_execute(mode, false))
        .then(argument("filter", ArgumentType::BlockState).executes(fill_execute(mode, true)))
}

fn fill_execute(
    mode: FillMode,
    filtered: bool,
) -> impl Fn(&CommandContext<Source>) -> Result<i32, CommandError> + Send + Sync + 'static {
    move |ctx| {
        let from = position(ctx, "from")?;
        let to = position(ctx, "to")?;
        let block = block_argument(ctx, "block")?;
        let filter = if filtered {
            Some(filter_argument(ctx)?)
        } else {
            None
        };
        dispatch(
            ctx.source(),
            WorldCommand::Fill {
                from,
                to,
                block,
                mode,
                filter,
            },
        )
    }
}

fn clone_tree() -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("clone")
        .requires(|s| s.permission >= 2)
        .then(
            argument("begin", ArgumentType::BlockPos).then(
                argument("end", ArgumentType::BlockPos).then(
                    argument("destination", ArgumentType::BlockPos)
                        .executes(clone_execute(CloneMode::Replace))
                        .then(literal("replace").executes(clone_execute(CloneMode::Replace)))
                        .then(literal("masked").executes(clone_execute(CloneMode::Masked))),
                ),
            ),
        )
}

fn clone_execute(
    mode: CloneMode,
) -> impl Fn(&CommandContext<Source>) -> Result<i32, CommandError> + Send + Sync + 'static {
    move |ctx| {
        let begin = position(ctx, "begin")?;
        let end = position(ctx, "end")?;
        let destination = position(ctx, "destination")?;
        dispatch(
            ctx.source(),
            WorldCommand::Clone {
                begin,
                end,
                destination,
                mode,
            },
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Block-state arguments (vanilla keys: `argument.block.id`,
// `argument.block.property.unknown`, `argument.block.property.invalid`).
// ---------------------------------------------------------------------------------------------

fn normalized(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn block_name(state: u32) -> String {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .map_or_else(|| state.to_string(), |(name, _)| normalized(name))
}

fn validate_block(id: &str, props: &[(String, String)]) -> Result<(), CommandError> {
    let probe = ferrite_data::blocks::default_state(id)
        .ok_or_else(|| CommandError::failed(format!("Unknown block: {id}")))?;
    let (name, declared) = ferrite_data::blocks::state(probe)
        .ok_or_else(|| CommandError::failed(format!("Unknown block: {id}")))?;
    for (key, _) in props {
        if !declared.iter().any(|(declared, _)| declared == key) {
            return Err(CommandError::failed(format!(
                "Block {name} does not have property '{key}'"
            )));
        }
    }
    for (key, value) in props {
        if ferrite_data::blocks::state_id(id, &[(key, value)]).is_none() {
            return Err(CommandError::failed(format!(
                "Block {name} with property '{key}' does not accept value '{value}'"
            )));
        }
    }
    Ok(())
}

pub fn resolve_block(value: &BlockStateValue) -> Result<u16, CommandError> {
    let id = normalized(&value.id);
    validate_block(&id, &value.properties)?;
    let props: Vec<(&str, &str)> = value
        .properties
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    ferrite_data::blocks::state_id(&id, &props)
        .ok_or_else(|| CommandError::failed(format!("Unknown block: {}", value.id)))
}

pub fn resolve_filter(value: &BlockStateValue) -> Result<BlockFilter, CommandError> {
    let id = normalized(&value.id);
    validate_block(&id, &value.properties)?;
    Ok(BlockFilter {
        id,
        props: value.properties.clone(),
    })
}

// ---------------------------------------------------------------------------------------------
// Execution (region thread).
// ---------------------------------------------------------------------------------------------

/// Runs a world command for a player and sends its feedback as chat.
pub fn run(player: &mut Player, command: WorldCommand) {
    let origin = Origin {
        pos: player.pos,
        yaw: player.yaw,
        pitch: player.pitch,
    };
    let Some(source) = &player.source else {
        feedback(player, "Chunk not loaded");
        return;
    };
    match execute(&command, &origin, &**source) {
        Ok(outcome) => {
            for change in outcome.changes {
                // Like a player-placed block, a command change notifies neighbours and the
                // region's redstone/gravity systems on the next tick.
                if player.change_block(change.location, None, change.state) {
                    let pos = BlockPos::unpack(change.location);
                    player.changed_blocks.push([pos.x, pos.y, pos.z]);
                }
            }
            feedback(player, &outcome.message);
        }
        Err(message) => feedback(player, &message),
    }
}

fn feedback(player: &Player, text: &str) {
    let _ = player.outbound.send(packet(&SystemChat {
        content: PlainText(text.into()),
        overlay: false,
    }));
}

pub fn execute(
    command: &WorldCommand,
    origin: &Origin,
    world: &dyn WorldBlocks,
) -> Result<Outcome, String> {
    match command {
        WorldCommand::SetBlock {
            pos,
            block,
            mode,
            nbt,
        } => set_block(resolve(pos, origin), *block, *mode, nbt.as_ref(), world),
        WorldCommand::Fill {
            from,
            to,
            block,
            mode,
            filter,
        } => fill(
            resolve(from, origin),
            resolve(to, origin),
            *block,
            *mode,
            filter.as_ref(),
            world,
        ),
        WorldCommand::Clone {
            begin,
            end,
            destination,
            mode,
        } => clone(
            resolve(begin, origin),
            resolve(end, origin),
            resolve(destination, origin),
            *mode,
            world,
        ),
    }
}

pub fn resolve(coords: &Coordinates, origin: &Origin) -> [i32; 3] {
    let [x, y, z] = match *coords {
        Coordinates::World([x, y, z]) => [
            x.resolve(origin.pos[0]),
            y.resolve(origin.pos[1]),
            z.resolve(origin.pos[2]),
        ],
        Coordinates::Local { left, up, forwards } => {
            let yaw = f64::from(origin.yaw).to_radians();
            let pitch = f64::from(origin.pitch).to_radians();
            let forward = [
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            ];
            let flat = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
            let (right_x, right_z) = if flat == 0.0 {
                (1.0, 0.0)
            } else {
                (forward[2] / flat, -forward[0] / flat)
            };
            [
                origin.pos[0] + forward[0] * forwards + right_x * left,
                origin.pos[1] + forward[1] * forwards + up,
                origin.pos[2] + forward[2] * forwards + right_z * left,
            ]
        }
    };
    [x.floor() as i32, y.floor() as i32, z.floor() as i32]
}

/// The chunk whose region owns the command's first block, for routing a world edit.
pub fn command_chunk(command: &WorldCommand, origin: &Origin) -> ChunkPos {
    let [x, _, z] = match command {
        WorldCommand::SetBlock { pos, .. } => resolve(pos, origin),
        WorldCommand::Fill { from, .. } => resolve(from, origin),
        WorldCommand::Clone { begin, .. } => resolve(begin, origin),
    };
    ChunkPos(x.div_euclid(16), z.div_euclid(16))
}

/// Applies a world command on the region owning its first block and sends each changed
/// block to every player that has it loaded. Console and function commands have no player
/// mailbox to run through, so they use this path.
pub fn apply_world(
    command: &WorldCommand,
    origin: &Origin,
    world: &dyn WorldBlocks,
    tracking: &Tracking,
) {
    let _ = apply_world_result(command, origin, world, tracking);
}

/// [`apply_world`], returning the feedback line (or the error) so a command block can store it.
pub fn apply_world_result(
    command: &WorldCommand,
    origin: &Origin,
    world: &dyn WorldBlocks,
    tracking: &Tracking,
) -> Result<Outcome, String> {
    let outcome = execute(command, origin, world)?;
    for change in &outcome.changes {
        let pos = BlockPos::unpack(change.location);
        world.place_block_entity([pos.x, pos.y, pos.z], change.state, None);
    }
    let tracking = tracking.lock().unwrap();
    for change in &outcome.changes {
        let pos = BlockPos::unpack(change.location);
        let chunk = (pos.x.div_euclid(16), pos.z.div_euclid(16));
        let update = packet(&BlockUpdate {
            location: change.location,
            block_state: VarInt(change.state as i32),
        });
        for (loaded, outbound) in tracking.values() {
            if loaded.contains(&chunk) {
                let _ = outbound.send(update.clone());
            }
        }
    }
    Ok(outcome)
}

fn bounds(a: [i32; 3], b: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    (
        [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
        [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
    )
}

fn volume(min: [i32; 3], max: [i32; 3]) -> u128 {
    let side = |a: i32, b: i32| u128::from(a.abs_diff(b)) + 1;
    side(min[0], max[0]) * side(min[1], max[1]) * side(min[2], max[2])
}

fn air() -> u16 {
    ferrite_data::blocks::default_state("minecraft:air").unwrap_or(0)
}

fn is_air(state: u32) -> bool {
    u16::try_from(state)
        .ok()
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, _)| {
            matches!(
                normalized(name).as_str(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
        })
}

fn set_block(
    pos: [i32; 3],
    block: u16,
    mode: SetMode,
    nbt: Option<&Tag>,
    world: &dyn WorldBlocks,
) -> Result<Outcome, String> {
    let [x, y, z] = pos;
    let old = world
        .block(x, y, z)
        .ok_or_else(|| "Chunk not loaded".to_string())?;
    if mode == SetMode::Keep
        && u16::try_from(old).is_ok_and(|state| !ferrite_data::block_props::get(state).replaceable)
    {
        return Err(format!(
            "Could not set block to {} as it is currently {}",
            block_name(u32::from(block)),
            block_name(old)
        ));
    }
    if nbt.is_some()
        && !ferrite_data::blocks::state(block)
            .is_some_and(|(name, _)| super::command_block::Mode::of_block(name).is_some())
    {
        return Err("The target block is not a block entity".into());
    }
    if !world.set_block(x, y, z, u32::from(block)) {
        return Err("Chunk not loaded".into());
    }
    if nbt.is_some() && !world.place_block_entity(pos, u32::from(block), nbt) {
        return Err("The target block is not a block entity".into());
    }
    Ok(Outcome {
        changes: vec![Change {
            location: BlockPos::new(x, y, z).pack(),
            state: u32::from(block),
        }],
        message: format!("Changed the block at {x}, {y}, {z}"),
    })
}

fn fill(
    from: [i32; 3],
    to: [i32; 3],
    block: u16,
    mode: FillMode,
    filter: Option<&BlockFilter>,
    world: &dyn WorldBlocks,
) -> Result<Outcome, String> {
    let (min, max) = bounds(from, to);
    let volume = volume(min, max);
    if volume > MAX_BLOCKS {
        return Err(format!(
            "Too many blocks in the specified area (maximum {MAX_BLOCKS}, but specified {volume})"
        ));
    }
    let mut changes = Vec::new();
    let mut count = 0u128;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                let old = world
                    .block(x, y, z)
                    .ok_or_else(|| "Chunk not loaded".to_string())?;
                if filter.is_some_and(|filter| !filter.matches(old)) {
                    continue;
                }
                let shell = x == min[0]
                    || x == max[0]
                    || y == min[1]
                    || y == max[1]
                    || z == min[2]
                    || z == max[2];
                let target = match mode {
                    FillMode::Replace | FillMode::Destroy => block,
                    FillMode::Keep if is_air(old) => block,
                    FillMode::Keep => continue,
                    FillMode::Hollow if shell => block,
                    FillMode::Hollow => air(),
                    FillMode::Outline if shell => block,
                    FillMode::Outline => continue,
                };
                count += 1;
                if old != u32::from(target) {
                    if !world.set_block(x, y, z, u32::from(target)) {
                        return Err("Chunk not loaded".into());
                    }
                    changes.push(Change {
                        location: BlockPos::new(x, y, z).pack(),
                        state: u32::from(target),
                    });
                }
            }
        }
    }
    if count == 0 {
        // `commands.fill.failed`
        return Err("No blocks were filled".into());
    }
    Ok(Outcome {
        changes,
        // `commands.fill.success`
        message: format!("Successfully filled {count} block(s)"),
    })
}

fn clone(
    begin: [i32; 3],
    end: [i32; 3],
    destination: [i32; 3],
    mode: CloneMode,
    world: &dyn WorldBlocks,
) -> Result<Outcome, String> {
    let (min, max) = bounds(begin, end);
    let volume = volume(min, max);
    if volume > MAX_BLOCKS {
        return Err(format!(
            "Too many blocks in the specified area (maximum {MAX_BLOCKS}, but specified {volume})"
        ));
    }
    // Snapshot first: source and destination may overlap.
    let mut source = Vec::with_capacity(volume as usize);
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                source.push(
                    world
                        .block(x, y, z)
                        .ok_or_else(|| "Chunk not loaded".to_string())?,
                );
            }
        }
    }
    let mut changes = Vec::new();
    let mut count = 0u128;
    let mut index = 0usize;
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                let state = source[index];
                index += 1;
                if mode == CloneMode::Masked && is_air(state) {
                    continue;
                }
                let tx = destination[0]
                    .checked_add(x - min[0])
                    .ok_or_else(|| "Invalid position".to_string())?;
                let ty = destination[1]
                    .checked_add(y - min[1])
                    .ok_or_else(|| "Invalid position".to_string())?;
                let tz = destination[2]
                    .checked_add(z - min[2])
                    .ok_or_else(|| "Invalid position".to_string())?;
                let old = world
                    .block(tx, ty, tz)
                    .ok_or_else(|| "Chunk not loaded".to_string())?;
                count += 1;
                if old != state {
                    if !world.set_block(tx, ty, tz, state) {
                        return Err("Chunk not loaded".into());
                    }
                    changes.push(Change {
                        location: BlockPos::new(tx, ty, tz).pack(),
                        state,
                    });
                }
            }
        }
    }
    if count == 0 {
        // `commands.clone.failed`
        return Err("No blocks were cloned".into());
    }
    Ok(Outcome {
        changes,
        // `commands.clone.success`
        message: format!("Successfully cloned {count} block(s)"),
    })
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_command::arguments::WorldCoordinate;
    use std::cell::RefCell;
    use std::collections::HashMap;

    fn source(permission: u8, actions: tokio::sync::mpsc::UnboundedSender<Action>) -> Source {
        Source {
            permission,
            origin: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            name: None,
            state: std::sync::Arc::new(std::sync::Mutex::new(
                super::super::commands::GameState::default(),
            )),
            actions,
            anchor: ferrite_command::EntityAnchor::Feet,
            dimension: crate::game::Dimension::Overworld,
            world: None,
            tree: None,
            function_depth: 0,
        }
    }

    struct TestWorld {
        blocks: RefCell<HashMap<(i32, i32, i32), u32>>,
    }

    impl TestWorld {
        fn new() -> Self {
            Self {
                blocks: RefCell::new(HashMap::new()),
            }
        }
        fn put(&self, x: i32, y: i32, z: i32, state: u16) {
            self.blocks.borrow_mut().insert((x, y, z), u32::from(state));
        }
        fn get(&self, x: i32, y: i32, z: i32) -> u32 {
            *self.blocks.borrow().get(&(x, y, z)).unwrap_or(&0)
        }
    }

    impl WorldBlocks for TestWorld {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u32> {
            Some(self.get(x, y, z))
        }
        fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool {
            self.blocks.borrow_mut().insert((x, y, z), state);
            true
        }
    }

    fn origin() -> Origin {
        Origin {
            pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    fn pos(x: f64, y: f64, z: f64) -> Coordinates {
        Coordinates::World([
            WorldCoordinate::absolute(x),
            WorldCoordinate::absolute(y),
            WorldCoordinate::absolute(z),
        ])
    }

    fn block(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).unwrap()
    }

    fn stone() -> u16 {
        block("minecraft:stone")
    }

    #[test]
    fn setblock_replaces_and_keeps_only_air() {
        let world = TestWorld::new();
        let outcome = execute(
            &WorldCommand::SetBlock {
                pos: pos(1.0, 64.0, 2.0),
                block: stone(),
                mode: SetMode::Replace,
                nbt: None,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(world.get(1, 64, 2), u32::from(stone()));
        assert_eq!(outcome.changes.len(), 1);
        assert_eq!(outcome.changes[0].location, BlockPos::new(1, 64, 2).pack());
        assert!(outcome.message.contains("Changed the block"));
        // `keep` refuses to overwrite a solid block.
        let error = execute(
            &WorldCommand::SetBlock {
                pos: pos(1.0, 64.0, 2.0),
                block: block("minecraft:dirt"),
                mode: SetMode::Keep,
                nbt: None,
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert!(error.contains("Could not set block"), "{error}");
        assert_eq!(world.get(1, 64, 2), u32::from(stone()));
        // ... but replaces air.
        execute(
            &WorldCommand::SetBlock {
                pos: pos(1.0, 65.0, 2.0),
                block: block("minecraft:dirt"),
                mode: SetMode::Keep,
                nbt: None,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(world.get(1, 65, 2), u32::from(block("minecraft:dirt")));
    }

    #[test]
    fn block_state_errors_name_the_offending_part() {
        let unknown = resolve_block(&BlockStateValue {
            id: "not_a_block".into(),
            properties: vec![],
            nbt: None,
        })
        .unwrap_err();
        assert_eq!(unknown.to_string(), "Unknown block: minecraft:not_a_block");
        let property = resolve_block(&BlockStateValue {
            id: "stone".into(),
            properties: vec![("facing".into(), "north".into())],
            nbt: None,
        })
        .unwrap_err();
        assert!(
            property
                .to_string()
                .contains("does not have property 'facing'")
        );
        let value = resolve_block(&BlockStateValue {
            id: "oak_stairs".into(),
            properties: vec![("half".into(), "sideways".into())],
            nbt: None,
        })
        .unwrap_err();
        assert!(
            value
                .to_string()
                .contains("does not accept value 'sideways'"),
            "{value}"
        );
        // Property values are honoured.
        assert_ne!(
            resolve_block(&BlockStateValue {
                id: "oak_stairs".into(),
                properties: vec![("half".into(), "top".into())],
                nbt: None,
            })
            .unwrap(),
            block("minecraft:oak_stairs")
        );
    }

    #[test]
    fn fill_replaces_keeps_hollows_and_outlines() {
        let world = TestWorld::new();
        let from = pos(0.0, 64.0, 0.0);
        let to = pos(1.0, 64.0, 1.0);
        let outcome = execute(
            &WorldCommand::Fill {
                from,
                to,
                block: stone(),
                mode: FillMode::Replace,
                filter: None,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(outcome.changes.len(), 4);
        for x in 0..=1 {
            for z in 0..=1 {
                assert_eq!(world.get(x, 64, z), u32::from(stone()));
            }
        }
        // `keep` only fills air.
        let air_only = execute(
            &WorldCommand::Fill {
                from,
                to,
                block: block("minecraft:dirt"),
                mode: FillMode::Keep,
                filter: None,
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert_eq!(air_only, "No blocks were filled");
        // `hollow` clears the interior.
        execute(
            &WorldCommand::Fill {
                from: pos(0.0, 64.0, 0.0),
                to: pos(2.0, 66.0, 2.0),
                block: block("minecraft:glass"),
                mode: FillMode::Hollow,
                filter: None,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(world.get(0, 66, 0), u32::from(block("minecraft:glass")));
        assert_eq!(world.get(1, 65, 1), 0);
        // `outline` leaves the interior untouched.
        world.put(1, 65, 1, block("minecraft:dirt"));
        execute(
            &WorldCommand::Fill {
                from: pos(0.0, 64.0, 0.0),
                to: pos(2.0, 66.0, 2.0),
                block: block("minecraft:glass"),
                mode: FillMode::Outline,
                filter: None,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(world.get(1, 65, 1), u32::from(block("minecraft:dirt")));
    }

    #[test]
    fn fill_respects_filters_and_the_volume_limit() {
        let world = TestWorld::new();
        world.put(0, 64, 0, stone());
        let outcome = execute(
            &WorldCommand::Fill {
                from: pos(0.0, 64.0, 0.0),
                to: pos(1.0, 64.0, 0.0),
                block: block("minecraft:glass"),
                mode: FillMode::Replace,
                filter: Some(
                    resolve_filter(&BlockStateValue {
                        id: "stone".into(),
                        properties: vec![],
                        nbt: None,
                    })
                    .unwrap(),
                ),
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(outcome.changes.len(), 1);
        assert_eq!(world.get(0, 64, 0), u32::from(block("minecraft:glass")));
        assert_eq!(world.get(1, 64, 0), 0);
        let too_many = execute(
            &WorldCommand::Fill {
                from: pos(0.0, 64.0, 0.0),
                to: pos(200.0, 64.0, 200.0),
                block: stone(),
                mode: FillMode::Replace,
                filter: None,
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert!(too_many.contains("Too many blocks"), "{too_many}");
    }

    #[test]
    fn clone_replaces_and_masks_air() {
        let world = TestWorld::new();
        world.put(0, 64, 0, stone());
        let outcome = execute(
            &WorldCommand::Clone {
                begin: pos(0.0, 64.0, 0.0),
                end: pos(1.0, 64.0, 0.0),
                destination: pos(0.0, 64.0, 3.0),
                mode: CloneMode::Replace,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(outcome.message, "Successfully cloned 2 block(s)");
        assert_eq!(world.get(0, 64, 3), u32::from(stone()));
        assert_eq!(world.get(1, 64, 3), 0);
        // Masked skips air entirely.
        let masked = execute(
            &WorldCommand::Clone {
                begin: pos(0.0, 64.0, 0.0),
                end: pos(1.0, 64.0, 0.0),
                destination: pos(0.0, 64.0, 5.0),
                mode: CloneMode::Masked,
            },
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(masked.changes.len(), 1);
        assert_eq!(masked.message, "Successfully cloned 1 block(s)");
        // All-air masked clone fails.
        let empty = execute(
            &WorldCommand::Clone {
                begin: pos(10.0, 64.0, 0.0),
                end: pos(10.0, 64.0, 0.0),
                destination: pos(12.0, 64.0, 0.0),
                mode: CloneMode::Masked,
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert_eq!(empty, "No blocks were cloned");
        let too_many = execute(
            &WorldCommand::Clone {
                begin: pos(0.0, 64.0, 0.0),
                end: pos(200.0, 64.0, 200.0),
                destination: pos(0.0, 64.0, 0.0),
                mode: CloneMode::Replace,
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert!(too_many.contains("Too many blocks"), "{too_many}");
    }

    #[test]
    fn dispatcher_parses_world_commands_and_permissions() {
        let tree = super::super::commands::dispatcher();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let operator = source(2, tx.clone());
        let guest = source(0, tx);
        tree.execute("setblock 1 2 3 stone", guest.clone())
            .unwrap_err();
        tree.execute("setblock 1 2 3 stone", operator.clone())
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::World(WorldCommand::SetBlock { block, .. }) if block == stone()
        ));
        tree.execute("fill 0 64 0 2 64 2 glass keep", operator.clone())
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::World(WorldCommand::Fill {
                mode: FillMode::Keep,
                filter: None,
                ..
            })
        ));
        tree.execute("fill 0 64 0 2 64 2 glass replace stone", operator.clone())
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::World(WorldCommand::Fill {
                filter: Some(_),
                ..
            })
        ));
        tree.execute("clone 0 64 0 1 64 1 4 64 4 masked", operator.clone())
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Action::World(WorldCommand::Clone {
                mode: CloneMode::Masked,
                ..
            })
        ));
        assert!(
            tree.execute("setblock 1 2 3 minecraft:not_a_block", guest)
                .is_err()
        );
    }

    #[test]
    fn setblock_carries_its_block_entity_tag() {
        let tree = super::super::commands::dispatcher();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let operator = source(2, tx);
        tree.execute(
            "setblock 1 2 3 minecraft:command_block{Command:\"say hi\"}",
            operator.clone(),
        )
        .unwrap();
        match rx.try_recv().unwrap() {
            Action::World(WorldCommand::SetBlock { nbt, .. }) => {
                let ferrite_nbt::Tag::Compound(map) =
                    nbt.expect("the tag is carried by the command")
                else {
                    panic!("compound tag");
                };
                assert_eq!(
                    map.get("Command"),
                    Some(&ferrite_nbt::Tag::String("say hi".into()))
                );
            }
            other => panic!("unexpected action: {other:?}"),
        }
        // A tag on a block without a block entity is rejected before anything changes.
        let error = execute(
            &WorldCommand::SetBlock {
                pos: pos(1.0, 64.0, 2.0),
                block: stone(),
                mode: SetMode::Replace,
                nbt: Some(ferrite_nbt::snbt::parse("{Command:\"x\"}").unwrap()),
            },
            &origin(),
            &TestWorld::new(),
        )
        .unwrap_err();
        assert!(error.contains("not a block entity"), "{error}");
    }
}
