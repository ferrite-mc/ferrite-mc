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

/// Which blocks of the source `/clone` copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneMode {
    Replace,
    Masked,
    /// `filtered <filter>`: only blocks the [`CloneOptions::filter`] matches.
    Filtered,
}

/// `force|move|normal` of `/clone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloneMove {
    /// Overlapping source and destination areas fail.
    #[default]
    Normal,
    /// Overlap is allowed.
    Force,
    /// The copied source blocks become air afterwards.
    Move,
}

/// The parts of `/clone` beyond the mask: filter, move mode, `strict` and dimensions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CloneOptions {
    pub filter: Option<BlockFilter>,
    pub mover: CloneMove,
    /// Accepted; no extracted data says what it changes (see COMMANDS-STATUS.md).
    pub strict: bool,
    /// `from <sourceDimension>` / `to <targetDimension>` when given.
    pub source_dimension: Option<String>,
    pub target_dimension: Option<String>,
}

/// A parsed block-state argument. Unspecified properties are wildcards, like vanilla's block
/// predicate in `fill ... replace <filter>`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockFilter {
    id: String,
    props: Vec<(String, String)>,
    /// The members of a `#tag` filter, resolved when the command is parsed.
    tag: Option<std::sync::Arc<std::collections::HashSet<String>>>,
}

impl BlockFilter {
    /// Whether `state` passes: the block (or a member of the tag) matches and every listed
    /// property has that value; unlisted properties are wildcards.
    pub fn matches(&self, state: u32) -> bool {
        let Ok(state) = u16::try_from(state) else {
            return false;
        };
        let Some((name, props)) = ferrite_data::blocks::state(state) else {
            return false;
        };
        let block = normalized(name);
        let block_matches = match &self.tag {
            Some(members) => members.contains(&block),
            None => block == self.id,
        };
        block_matches
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
        options: CloneOptions,
    },
    /// `/place template`: blocks relative to `pos`, already rotated, mirrored and thinned.
    Template {
        pos: Coordinates,
        name: String,
        blocks: std::sync::Arc<Vec<([i32; 3], u16)>>,
        /// Block entities (relative position, full NBT) that belong to the blocks.
        entities: std::sync::Arc<Vec<([i32; 3], ferrite_nbt::Compound)>>,
        /// Mobs and other entities of the template, spawned by the world loop (`mod.rs`) in the
        /// chunk each one stands in; positions are relative to `pos`.
        spawns: std::sync::Arc<Vec<super::structure_template::TemplateEntity>>,
        /// `strict` (`StructurePlaceSettings.knownShape`): leave the placed blocks as stored. A
        /// non-strict placement fixes up the template's edges (see [`fix_template_edges`]).
        strict: bool,
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
    fn dimension(&self) -> super::Dimension;
    fn world_id(&self) -> super::WorldId {
        super::WorldId::Vanilla(self.dimension())
    }
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u32>;
    fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool;
    /// Creates the block entity a freshly placed block needs: command blocks, chests, furnaces
    /// and signs. `nbt` is the `/setblock` tag, which wins over the defaults.
    fn place_block_entity(&self, _pos: [i32; 3], _state: u32, _nbt: Option<&Tag>) -> bool {
        false
    }
    /// The block entity at a position, as saved by vanilla (`/clone` copies it).
    fn block_entity_at(&self, _pos: [i32; 3]) -> Option<ferrite_nbt::Compound> {
        None
    }
    /// Stores a block entity as saved by vanilla (its `id` names the type); false when the
    /// world cannot hold it.
    fn set_block_entity(&self, _pos: [i32; 3], _data: &ferrite_nbt::Compound) -> bool {
        false
    }
}

impl WorldBlocks for super::anvil::AnvilChunkSource {
    fn dimension(&self) -> super::Dimension {
        super::anvil::AnvilChunkSource::dimension(self)
    }
    fn world_id(&self) -> super::WorldId {
        super::anvil::AnvilChunkSource::world_id(self)
    }
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        super::anvil::AnvilChunkSource::block(self, x, y, z)
    }
    fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool {
        super::anvil::AnvilChunkSource::set_block(self, x, y, z, state).is_ok()
    }
    fn block_entity_at(&self, pos: [i32; 3]) -> Option<ferrite_nbt::Compound> {
        self.block_entity(pos[0], pos[1], pos[2])
            .map(|entity| entity.data)
    }
    fn set_block_entity(&self, pos: [i32; 3], data: &ferrite_nbt::Compound) -> bool {
        let Some(Tag::String(id)) = data.get("id") else {
            return false;
        };
        let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", id)
            .and_then(|id| u32::try_from(id).ok())
        else {
            return false;
        };
        let mut data = data.clone();
        data.insert("x".into(), Tag::Int(pos[0]));
        data.insert("y".into(), Tag::Int(pos[1]));
        data.insert("z".into(), Tag::Int(pos[2]));
        self.update_block_entity(pos[0], pos[1], pos[2], |slot| {
            *slot = Some(ferrite_world::BlockEntity { type_id, data });
            (true, ())
        })
        .is_ok()
    }
    fn place_block_entity(&self, pos: [i32; 3], state: u32, nbt: Option<&Tag>) -> bool {
        let Some((name, _)) = u16::try_from(state)
            .ok()
            .and_then(ferrite_data::blocks::state)
        else {
            return false;
        };
        if super::command_block::Mode::of_block(name).is_none() {
            return match plain_block_entity_id(name) {
                Some(id) => place_plain_block_entity(self, pos, &id, nbt),
                None => false,
            };
        }
        match nbt {
            Some(Tag::Compound(data)) => super::command_block::place_with(self, pos, name, data),
            _ => super::command_block::ensure(self, pos, name).is_some(),
        }
    }
}

/// The block entity type of a block that keeps its data in plain NBT (containers, furnaces and
/// signs), or `None` for a block without one. Command blocks have their own module.
fn plain_block_entity_id(block: &str) -> Option<String> {
    let name = block.strip_prefix("minecraft:")?;
    let id = match name {
        "chest" | "trapped_chest" | "furnace" | "blast_furnace" | "smoker" => name,
        _ if name.ends_with("_hanging_sign") => "hanging_sign",
        _ if name.ends_with("_sign") => "sign",
        _ => return None,
    };
    Some(format!("minecraft:{id}"))
}

/// Whether placing `block` creates a block entity that `/setblock ... {nbt}` can fill.
fn carries_block_entity(block: &str) -> bool {
    super::command_block::Mode::of_block(block).is_some() || plain_block_entity_id(block).is_some()
}

/// Creates the block entity of a placed chest, furnace or sign. With a tag the entity is
/// replaced by it (the tag wins, like `/setblock`); without one an existing entity is kept and
/// a missing one is created empty, as vanilla does when the block is placed.
fn place_plain_block_entity(
    world: &super::anvil::AnvilChunkSource,
    pos: [i32; 3],
    id: &str,
    nbt: Option<&Tag>,
) -> bool {
    let mut data = match nbt {
        Some(Tag::Compound(tag)) => tag.clone(),
        Some(_) => return false,
        None => ferrite_nbt::Compound::new(),
    };
    let Some(type_id) = ferrite_data::registries::id("minecraft:block_entity_type", id)
        .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    data.insert("id".into(), Tag::String(id.to_owned()));
    data.insert("x".into(), Tag::Int(pos[0]));
    data.insert("y".into(), Tag::Int(pos[1]));
    data.insert("z".into(), Tag::Int(pos[2]));
    let replace = nbt.is_some();
    world
        .update_block_entity(pos[0], pos[1], pos[2], |slot| {
            if slot.is_some() && !replace {
                return (false, ());
            }
            *slot = Some(ferrite_world::BlockEntity { type_id, data });
            (true, ())
        })
        .is_ok()
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
                    .then(literal("keep").executes(setblock_execute(SetMode::Keep)))
                    // `strict` is in the 26.3 tree; what it changes is not in the extracted
                    // data, so it behaves like `replace` (see COMMANDS-STATUS.md).
                    .then(literal("strict").executes(setblock_execute(SetMode::Replace))),
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
    // commands.json: `<block> [destroy|hollow|keep|outline|strict|replace [<filter> [destroy|
    // hollow|outline|strict]]]`. `strict` is accepted and behaves like `replace`.
    let filtered = |name: &'static str, mode: FillMode| {
        literal::<Source>(name).executes(fill_execute(mode, true))
    };
    fill_root(
        literal::<Source>("replace")
            .executes(fill_execute(FillMode::Replace, false))
            .then(
                argument("filter", ArgumentType::BlockPredicate)
                    .executes(fill_execute(FillMode::Replace, true))
                    .then(filtered("destroy", FillMode::Destroy))
                    .then(filtered("hollow", FillMode::Hollow))
                    .then(filtered("outline", FillMode::Outline))
                    .then(filtered("strict", FillMode::Replace)),
            ),
    )
}

fn fill_root(
    replace: ferrite_command::NodeBuilder<Source>,
) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("fill")
        .requires(|s| s.permission >= 2)
        .then(
            argument("from", ArgumentType::BlockPos).then(
                argument("to", ArgumentType::BlockPos).then(
                    argument("block", ArgumentType::BlockState)
                        .executes(fill_execute(FillMode::Replace, false))
                        .then(replace)
                        .then(literal("destroy").executes(fill_execute(FillMode::Destroy, false)))
                        .then(literal("keep").executes(fill_execute(FillMode::Keep, false)))
                        .then(literal("hollow").executes(fill_execute(FillMode::Hollow, false)))
                        .then(literal("outline").executes(fill_execute(FillMode::Outline, false)))
                        .then(literal("strict").executes(fill_execute(FillMode::Replace, false))),
                ),
            ),
        )
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

/// `[force|move|normal]` leaves below a mask node.
fn clone_movers(
    node: ferrite_command::NodeBuilder<Source>,
    mode: CloneMode,
    strict: bool,
    with_dimensions: bool,
) -> ferrite_command::NodeBuilder<Source> {
    let run = move |mover: CloneMove| clone_execute(mode, mover, strict, with_dimensions);
    node.executes(run(CloneMove::Normal))
        .then(literal("force").executes(run(CloneMove::Force)))
        .then(literal("move").executes(run(CloneMove::Move)))
        .then(literal("normal").executes(run(CloneMove::Normal)))
}

/// `filtered|masked|replace` below a destination (with `strict` as an optional prefix).
fn clone_masks(strict: bool, with_dimensions: bool) -> Vec<ferrite_command::NodeBuilder<Source>> {
    vec![
        literal::<Source>("filtered").then(clone_movers(
            argument("filter", ArgumentType::BlockPredicate),
            CloneMode::Filtered,
            strict,
            with_dimensions,
        )),
        clone_movers(
            literal("masked"),
            CloneMode::Masked,
            strict,
            with_dimensions,
        ),
        clone_movers(
            literal("replace"),
            CloneMode::Replace,
            strict,
            with_dimensions,
        ),
    ]
}

fn clone_destination(with_dimensions: bool) -> ferrite_command::NodeBuilder<Source> {
    let mut destination =
        argument::<Source>("destination", ArgumentType::BlockPos).executes(clone_execute(
            CloneMode::Replace,
            CloneMove::Normal,
            false,
            with_dimensions,
        ));
    for mask in clone_masks(false, with_dimensions) {
        destination = destination.then(mask);
    }
    let mut strict = literal::<Source>("strict").executes(clone_execute(
        CloneMode::Replace,
        CloneMove::Normal,
        true,
        with_dimensions,
    ));
    for mask in clone_masks(true, with_dimensions) {
        strict = strict.then(mask);
    }
    destination.then(strict)
}

fn clone_tree() -> ferrite_command::NodeBuilder<Source> {
    // commands.json: `clone <begin> <end> <destination> [...]`, `... to <targetDimension>
    // <destination> [...]` and `clone from <sourceDimension> <begin> <end> ...`.
    let area = |with_dimensions: bool| {
        argument::<Source>("begin", ArgumentType::BlockPos).then(
            argument::<Source>("end", ArgumentType::BlockPos)
                .then(clone_destination(with_dimensions))
                .then(
                    literal::<Source>("to").then(
                        argument::<Source>("targetDimension", ArgumentType::Dimension)
                            .then(clone_destination(true)),
                    ),
                ),
        )
    };
    literal::<Source>("clone")
        .requires(|s| s.permission >= 2)
        .then(area(false))
        .then(
            literal::<Source>("from").then(
                argument::<Source>("sourceDimension", ArgumentType::Dimension).then(area(true)),
            ),
        )
}

fn clone_execute(
    mode: CloneMode,
    mover: CloneMove,
    strict: bool,
    _with_dimensions: bool,
) -> impl Fn(&CommandContext<Source>) -> Result<i32, CommandError> + Send + Sync + 'static {
    move |ctx| {
        let begin = position(ctx, "begin")?;
        let end = position(ctx, "end")?;
        let destination = position(ctx, "destination")?;
        let filter = if mode == CloneMode::Filtered {
            Some(filter_argument(ctx)?)
        } else {
            None
        };
        let dimension = |name: &str| ctx.string(name).map(str::to_owned);
        dispatch(
            ctx.source(),
            WorldCommand::Clone {
                begin,
                end,
                destination,
                mode,
                options: CloneOptions {
                    filter,
                    mover,
                    strict,
                    source_dimension: dimension("sourceDimension"),
                    target_dimension: dimension("targetDimension"),
                },
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
    if let Some(tag) = value.id.strip_prefix('#') {
        // `arguments.block.tag.unknown`
        let members = super::block_tags::members(tag).ok_or_else(|| {
            CommandError::failed(format!("Unknown block tag '{}'", normalized(tag)))
        })?;
        return Ok(BlockFilter {
            id: value.id.clone(),
            props: value.properties.clone(),
            tag: Some(members),
        });
    }
    let id = normalized(&value.id);
    validate_block(&id, &value.properties)?;
    Ok(BlockFilter {
        id,
        props: value.properties.clone(),
        tag: None,
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
            options,
        } => clone(
            resolve(begin, origin),
            resolve(end, origin),
            resolve(destination, origin),
            *mode,
            options,
            world,
        ),
        WorldCommand::Template {
            pos,
            name,
            blocks,
            entities,
            strict,
            ..
        } => {
            let [ox, oy, oz] = resolve(pos, origin);
            let mut changes = Vec::new();
            let mut placed = std::collections::HashSet::new();
            for ([x, y, z], state) in blocks.iter() {
                let (x, y, z) = (ox + x, oy + y, oz + z);
                if world.block(x, y, z).is_some() && world.set_block(x, y, z, u32::from(*state)) {
                    placed.insert([x, y, z]);
                    changes.push(Change {
                        location: BlockPos::new(x, y, z).pack(),
                        state: u32::from(*state),
                    });
                }
            }
            if changes.is_empty() {
                return Err("Chunk not loaded".into());
            }
            if !strict {
                fix_template_edges(world, &placed, &mut changes);
            }
            for ([x, y, z], data) in entities.iter() {
                world.set_block_entity([ox + x, oy + y, oz + z], data);
            }
            Ok(Outcome {
                changes,
                message: if name.starts_with("feature ")
                    || name.starts_with("structure ")
                    || name.starts_with("jigsaw ")
                {
                    format!("Placed {name} at {ox}, {oy}, {oz}")
                } else {
                    format!("Loaded template '{name}' at {ox}, {oy}, {oz}")
                },
            })
        }
    }
}

/// Support rules of a block world for [`neighbors::can_survive`](super::neighbors::can_survive).
struct Support<'a, W: WorldBlocks + ?Sized>(&'a W);

impl<W: WorldBlocks + ?Sized> super::neighbors::BlockWorld for Support<'_, W> {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        self.0
            .block(x, y, z)
            .and_then(|state| u16::try_from(state).ok())
    }
}

/// The non-strict part of `StructureTemplate.placeInWorld`, once every block is written:
///
/// 1. `updateShapeAtEdge`: for every face between a placed block and a position the template did
///    not place (`DiscreteVoxelShape.forAllFaces`), both blocks get `updateShape` for that face;
/// 2. for every placed block, `Block.updateFromNeighbourShapes` (all six neighbours).
///
/// Ferrite's only shape rule is support: `updateShape` of a block whose `canSurvive` fails gives
/// air (torches, buttons, rails, pressure plates, redstone wire ... in
/// [`neighbors::can_survive`](super::neighbors::can_survive)). That is what is reproduced here.
/// The rules that only change a block's *state* (fence, wall, pane, stairs and redstone-wire
/// connections, door halves, chest halves) do not exist in this server yet, so those blocks keep
/// the state the template stored. The `updateNeighborsAt` block updates (redstone) are not sent.
fn fix_template_edges<W: WorldBlocks + ?Sized>(
    world: &W,
    placed: &std::collections::HashSet<[i32; 3]>,
    changes: &mut Vec<Change>,
) {
    use ferrite_math::Direction;
    const ORDER: [Direction; 6] = [
        Direction::West,
        Direction::East,
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
    ];
    let Some(air) = ferrite_data::blocks::default_state("air") else {
        return;
    };
    let remove_if_unsupported = |at: [i32; 3], changes: &mut Vec<Change>| {
        let Some(state) = world
            .block(at[0], at[1], at[2])
            .and_then(|state| u16::try_from(state).ok())
        else {
            return;
        };
        if !super::neighbors::can_survive(state, at, &Support(world))
            && world.set_block(at[0], at[1], at[2], u32::from(air))
        {
            changes.push(Change {
                location: BlockPos::new(at[0], at[1], at[2]).pack(),
                state: u32::from(air),
            });
        }
    };
    // The placed positions in a stable order, so a run is reproducible.
    let mut order: Vec<[i32; 3]> = placed.iter().copied().collect();
    order.sort_unstable();
    for at in &order {
        for direction in ORDER {
            let (dx, dy, dz) = direction.step();
            let outside = [at[0] + dx, at[1] + dy, at[2] + dz];
            if placed.contains(&outside) {
                continue;
            }
            remove_if_unsupported(*at, changes);
            remove_if_unsupported(outside, changes);
        }
    }
    for at in &order {
        remove_if_unsupported(*at, changes);
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

/// The entities a `/place template` command spawns, at their absolute positions. A template
/// entity of an unknown type is dropped, as a saved one is when a chunk loads.
pub fn template_spawns(
    command: &WorldCommand,
    origin: &Origin,
) -> Vec<([f64; 3], ferrite_entity::EntityRecord)> {
    let WorldCommand::Template { pos, spawns, .. } = command else {
        return Vec::new();
    };
    let base = resolve(pos, origin);
    spawns
        .iter()
        .filter_map(|spawn| {
            let at = [
                f64::from(base[0]) + spawn.pos[0],
                f64::from(base[1]) + spawn.pos[1],
                f64::from(base[2]) + spawn.pos[2],
            ];
            let mut nbt = spawn.nbt.clone();
            nbt.insert(
                "Pos".into(),
                ferrite_nbt::Tag::List(ferrite_nbt::List::Double(at.to_vec())),
            );
            super::entity_save::entity_from_nbt(&nbt).map(|record| (at, record))
        })
        .collect()
}

/// The chunk whose region owns the command's first block, for routing a world edit.
pub fn command_chunk(command: &WorldCommand, origin: &Origin) -> ChunkPos {
    let [x, _, z] = match command {
        WorldCommand::SetBlock { pos, .. } => resolve(pos, origin),
        WorldCommand::Fill { from, .. } => resolve(from, origin),
        WorldCommand::Clone { begin, .. } => resolve(begin, origin),
        WorldCommand::Template { pos, .. } => resolve(pos, origin),
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
        super::region::broadcast_tracked_chunk(&tracking, world.world_id(), chunk, &update);
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

pub(super) fn is_air(state: u32) -> bool {
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
        && !ferrite_data::blocks::state(block).is_some_and(|(name, _)| carries_block_entity(name))
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

/// One block `/clone` read from the source area.
#[derive(Debug, Clone)]
pub struct ClonedBlock {
    pub at: [i32; 3],
    pub state: u32,
    pub entity: Option<ferrite_nbt::Compound>,
}

/// The blocks read from a source area, ready to be written (possibly into another dimension).
#[derive(Debug, Clone)]
pub struct CloneRead {
    pub min: [i32; 3],
    pub copied: Vec<ClonedBlock>,
}

/// The size and overlap checks vanilla runs before a `/clone` reads anything. Same- and
/// cross-dimension clones share them: `from <dim>`/`to <dim>` can still name the executing
/// dimension on both sides (for example `execute in the_nether run clone ... from overworld to
/// overworld`), in which case the areas really can overlap.
pub fn validate_clone(
    begin: [i32; 3],
    end: [i32; 3],
    destination: [i32; 3],
    options: &CloneOptions,
) -> Result<(), String> {
    let (min, max) = bounds(begin, end);
    let area = volume(min, max);
    if area > MAX_BLOCKS {
        return Err(format!(
            "Too many blocks in the specified area (maximum {MAX_BLOCKS}, but specified {area})"
        ));
    }
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let mut destination_max = [0; 3];
    for axis in 0..3 {
        destination_max[axis] = destination[axis]
            .checked_add(size[axis])
            .ok_or_else(|| "Invalid position".to_string())?;
    }
    // `commands.clone.overlap`: overlapping areas fail unless `force` or the clone is a no-op.
    let overlap =
        (0..3).all(|axis| min[axis] <= destination_max[axis] && destination[axis] <= max[axis]);
    if overlap && options.mover != CloneMove::Force {
        return Err("The source and destination areas cannot overlap".into());
    }
    Ok(())
}

/// Reads the blocks of `begin..=end` that `mode` selects (`commands.clone.toobig` and the
/// snapshot step of vanilla's `CloneCommands`).
pub fn clone_read(
    begin: [i32; 3],
    end: [i32; 3],
    mode: CloneMode,
    options: &CloneOptions,
    world: &dyn WorldBlocks,
) -> Result<CloneRead, String> {
    let (min, max) = bounds(begin, end);
    let volume = volume(min, max);
    if volume > MAX_BLOCKS {
        return Err(format!(
            "Too many blocks in the specified area (maximum {MAX_BLOCKS}, but specified {volume})"
        ));
    }
    // Snapshot first: source and destination may overlap (`force`).
    let mut copied = Vec::new();
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                let state = world
                    .block(x, y, z)
                    .ok_or_else(|| "Chunk not loaded".to_string())?;
                let take = match mode {
                    CloneMode::Replace => true,
                    CloneMode::Masked => !is_air(state),
                    CloneMode::Filtered => options
                        .filter
                        .as_ref()
                        .is_some_and(|filter| filter.matches(state)),
                };
                if take {
                    copied.push(ClonedBlock {
                        at: [x, y, z],
                        state,
                        entity: world.block_entity_at([x, y, z]),
                    });
                }
            }
        }
    }
    Ok(CloneRead { min, copied })
}

fn note_change(at: [i32; 3], state: u32, changes: &mut Vec<Change>) {
    let location = BlockPos::new(at[0], at[1], at[2]).pack();
    changes.retain(|change: &Change| change.location != location);
    changes.push(Change { location, state });
}

/// `move`: the copied source blocks become air.
pub fn clone_clear_source(
    read: &CloneRead,
    world: &dyn WorldBlocks,
) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    for block in &read.copied {
        let old = world.block(block.at[0], block.at[1], block.at[2]);
        if old != Some(u32::from(air())) {
            if !world.set_block(block.at[0], block.at[1], block.at[2], u32::from(air())) {
                return Err("Chunk not loaded".into());
            }
            note_change(block.at, u32::from(air()), &mut changes);
        }
    }
    Ok(changes)
}

/// Writes a snapshot at `destination` (the minimum corner), block entities included.
pub fn clone_write(
    read: &CloneRead,
    destination: [i32; 3],
    world: &dyn WorldBlocks,
    changes: &mut Vec<Change>,
) -> Result<u128, String> {
    let mut count = 0u128;
    let mut touched = std::collections::BTreeMap::new();
    for block in &read.copied {
        let target = [
            destination[0] + block.at[0] - read.min[0],
            destination[1] + block.at[1] - read.min[1],
            destination[2] + block.at[2] - read.min[2],
        ];
        let old = world
            .block(target[0], target[1], target[2])
            .ok_or_else(|| "Chunk not loaded".to_string())?;
        count += 1;
        if old != block.state || block.entity.is_some() {
            if !world.set_block(target[0], target[1], target[2], block.state) {
                return Err("Chunk not loaded".into());
            }
            note_change(target, block.state, changes);
        }
        touched.insert(target, block.entity.clone());
    }
    for (target, entity) in touched {
        if let Some(mut data) = entity {
            data.insert("x".into(), Tag::Int(target[0]));
            data.insert("y".into(), Tag::Int(target[1]));
            data.insert("z".into(), Tag::Int(target[2]));
            world.set_block_entity(target, &data);
        }
    }
    Ok(count)
}

/// The hand-off of a `/clone` between dimensions: the read happened on the source dimension's
/// region, the write is scheduled on the destination's, and `move` clears the source last.
#[derive(Debug, Clone)]
pub struct CrossClone {
    pub read: Result<CloneRead, String>,
    pub destination: [i32; 3],
    pub source_dimension: super::Dimension,
    pub target_dimension: super::Dimension,
    pub mover: CloneMove,
    /// The player's chat, when a player ran the command.
    pub reply: Option<ferrite_net::game::OutboundSender>,
}

/// Sends the changed blocks of a clone to the players tracking them.
fn broadcast_changes(changes: &[Change], world: &dyn WorldBlocks, tracking: &Tracking) {
    for change in changes {
        let pos = BlockPos::unpack(change.location);
        world.place_block_entity([pos.x, pos.y, pos.z], change.state, None);
    }
    let tracking = tracking.lock().unwrap();
    for change in changes {
        let pos = BlockPos::unpack(change.location);
        let update = packet(&BlockUpdate {
            location: change.location,
            block_state: VarInt(change.state as i32),
        });
        super::region::broadcast_tracked_chunk(
            &tracking,
            world.world_id(),
            (pos.x.div_euclid(16), pos.z.div_euclid(16)),
            &update,
        );
    }
}

/// The destination half of a cross-dimension clone, run on the destination's region.
pub fn apply_clone_write(
    read: &CloneRead,
    destination: [i32; 3],
    world: &dyn WorldBlocks,
    tracking: &Tracking,
) -> Result<Outcome, String> {
    let mut changes = Vec::new();
    let count = clone_write(read, destination, world, &mut changes)?;
    if count == 0 {
        return Err("No blocks were cloned".into());
    }
    broadcast_changes(&changes, world, tracking);
    Ok(Outcome {
        changes,
        message: format!("Successfully cloned {count} block(s)"),
    })
}

/// The source half of a cross-dimension `move`, run on the source's region.
pub fn apply_clone_clear(
    read: &CloneRead,
    world: &dyn WorldBlocks,
    tracking: &Tracking,
) -> Result<(), String> {
    let changes = clone_clear_source(read, world)?;
    broadcast_changes(&changes, world, tracking);
    Ok(())
}

fn clone(
    begin: [i32; 3],
    end: [i32; 3],
    destination: [i32; 3],
    mode: CloneMode,
    options: &CloneOptions,
    world: &dyn WorldBlocks,
) -> Result<Outcome, String> {
    // A clone between dimensions is split by the world loop into a read on the source
    // dimension's region and a write on the destination's (`clone_read`/`clone_write`);
    // by the time a command reaches this function both areas are in `world`.
    let here = world.dimension().key();
    for named in [&options.source_dimension, &options.target_dimension]
        .into_iter()
        .flatten()
    {
        if named != here {
            return Err(format!(
                "Cloning between dimensions needs the world loop ({named} is not the command's dimension {here})"
            ));
        }
    }
    validate_clone(begin, end, destination, options)?;
    let read = clone_read(begin, end, mode, options, world)?;
    let mut changes = if options.mover == CloneMove::Move {
        clone_clear_source(&read, world)?
    } else {
        Vec::new()
    };
    let count = clone_write(&read, destination, world, &mut changes)?;
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
            admin: None,
            tree: None,
            function_depth: 0,
        }
    }

    struct TestWorld {
        blocks: RefCell<HashMap<(i32, i32, i32), u32>>,
        entities: RefCell<Vec<([i32; 3], ferrite_nbt::Compound)>>,
    }

    impl TestWorld {
        fn new() -> Self {
            Self {
                blocks: RefCell::new(HashMap::new()),
                entities: RefCell::new(Vec::new()),
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
        fn dimension(&self) -> super::super::Dimension {
            super::super::Dimension::Overworld
        }
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u32> {
            Some(self.get(x, y, z))
        }
        fn set_block(&self, x: i32, y: i32, z: i32, state: u32) -> bool {
            self.blocks.borrow_mut().insert((x, y, z), state);
            true
        }
        fn block_entity_at(&self, pos: [i32; 3]) -> Option<ferrite_nbt::Compound> {
            self.entities
                .borrow()
                .iter()
                .rev()
                .find(|(at, _)| *at == pos)
                .map(|(_, data)| data.clone())
        }
        fn set_block_entity(&self, pos: [i32; 3], data: &ferrite_nbt::Compound) -> bool {
            self.entities.borrow_mut().push((pos, data.clone()));
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
    fn setblock_updates_only_the_command_world() {
        let (overworld, mut overworld_rx) = ferrite_net::game::OutboundSender::channel(1024);
        let (nether, mut nether_rx) = ferrite_net::game::OutboundSender::channel(1024);
        let tracking = Tracking::default();
        {
            let mut players = tracking.lock().unwrap();
            players.insert(
                uuid::Uuid::from_u128(1),
                (
                    super::super::WorldId::Vanilla(super::super::Dimension::Overworld),
                    [(0, 0)].into(),
                    overworld,
                ),
            );
            players.insert(
                uuid::Uuid::from_u128(2),
                (
                    super::super::WorldId::Vanilla(super::super::Dimension::Nether),
                    [(0, 0)].into(),
                    nether,
                ),
            );
        }
        apply_world_result(
            &WorldCommand::SetBlock {
                pos: pos(1.0, 64.0, 2.0),
                block: stone(),
                mode: SetMode::Replace,
                nbt: None,
            },
            &origin(),
            &TestWorld::new(),
            &tracking,
        )
        .unwrap();
        assert!(overworld_rx.try_recv().is_some());
        assert!(nether_rx.try_recv().is_none());
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
                options: CloneOptions::default(),
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
                options: CloneOptions::default(),
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
                options: CloneOptions::default(),
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
                options: CloneOptions::default(),
            },
            &origin(),
            &world,
        )
        .unwrap_err();
        assert!(too_many.contains("Too many blocks"), "{too_many}");
    }

    fn clone_with(
        world: &TestWorld,
        begin: (f64, f64, f64),
        end: (f64, f64, f64),
        destination: (f64, f64, f64),
        mode: CloneMode,
        options: CloneOptions,
    ) -> Result<Outcome, String> {
        execute(
            &WorldCommand::Clone {
                begin: pos(begin.0, begin.1, begin.2),
                end: pos(end.0, end.1, end.2),
                destination: pos(destination.0, destination.1, destination.2),
                mode,
                options,
            },
            &origin(),
            world,
        )
    }

    #[test]
    fn clone_validation_is_shared_with_the_cross_dimension_path() {
        // Overlap is rejected unless `force`, even when the two areas are named in another
        // dimension than the executor's.
        let overlap = CloneOptions::default();
        assert_eq!(
            validate_clone([0, 64, 0], [1, 64, 0], [1, 64, 0], &overlap).unwrap_err(),
            "The source and destination areas cannot overlap"
        );
        let force = CloneOptions {
            mover: CloneMove::Force,
            ..CloneOptions::default()
        };
        validate_clone([0, 64, 0], [1, 64, 0], [1, 64, 0], &force).unwrap();
        assert!(
            validate_clone([0, 64, 0], [200, 64, 200], [0, 64, 0], &force)
                .unwrap_err()
                .contains("Too many blocks")
        );
    }

    #[test]
    fn clone_overlap_force_move_and_filter() {
        let world = TestWorld::new();
        let log = block("minecraft:oak_log");
        world.put(0, 64, 0, stone());
        world.put(1, 64, 0, log);
        // Normal clone of overlapping areas fails like vanilla's `commands.clone.overlap`.
        let error = clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (1.0, 64.0, 0.0),
            (1.0, 64.0, 0.0),
            CloneMode::Replace,
            CloneOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error, "The source and destination areas cannot overlap");
        assert_eq!(world.get(2, 64, 0), 0, "nothing was written");
        // `force` allows it and reads the source before writing.
        clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (1.0, 64.0, 0.0),
            (1.0, 64.0, 0.0),
            CloneMode::Replace,
            CloneOptions {
                mover: CloneMove::Force,
                ..CloneOptions::default()
            },
        )
        .unwrap();
        assert_eq!(world.get(1, 64, 0), u32::from(stone()));
        assert_eq!(world.get(2, 64, 0), u32::from(log));
        // `filtered #logs` copies only the log.
        let tag = resolve_filter(&BlockStateValue {
            id: "#minecraft:logs".into(),
            properties: vec![],
            nbt: None,
        })
        .unwrap();
        let outcome = clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (2.0, 64.0, 0.0),
            (0.0, 64.0, 5.0),
            CloneMode::Filtered,
            CloneOptions {
                filter: Some(tag),
                ..CloneOptions::default()
            },
        )
        .unwrap();
        assert_eq!(outcome.message, "Successfully cloned 1 block(s)");
        assert_eq!(world.get(2, 64, 5), u32::from(log));
        assert_eq!(world.get(0, 64, 5), 0);
        // `move` clears the copied source blocks.
        clone_with(
            &world,
            (1.0, 64.0, 0.0),
            (2.0, 64.0, 0.0),
            (1.0, 64.0, 8.0),
            CloneMode::Replace,
            CloneOptions {
                mover: CloneMove::Move,
                ..CloneOptions::default()
            },
        )
        .unwrap();
        assert_eq!(world.get(1, 64, 8), u32::from(stone()));
        assert_eq!(world.get(1, 64, 0), 0);
        assert_eq!(world.get(2, 64, 0), 0);
    }

    #[test]
    fn clone_copies_block_entities_and_rejects_other_dimensions() {
        let world = TestWorld::new();
        world.put(0, 64, 0, block("minecraft:chest"));
        let mut data = ferrite_nbt::Compound::new();
        data.insert("id".into(), Tag::String("minecraft:chest".into()));
        data.insert("CustomName".into(), Tag::String("loot".into()));
        world.set_block_entity([0, 64, 0], &data);
        clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (0.0, 64.0, 0.0),
            (4.0, 64.0, 0.0),
            CloneMode::Replace,
            CloneOptions::default(),
        )
        .unwrap();
        let copied = world
            .block_entity_at([4, 64, 0])
            .expect("block entity copied");
        assert_eq!(copied.get("CustomName"), Some(&Tag::String("loot".into())));
        assert_eq!(copied.get("x"), Some(&Tag::Int(4)));
        // A dimension other than the command's is an honest error, not a silent clone.
        let error = clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (0.0, 64.0, 0.0),
            (8.0, 64.0, 0.0),
            CloneMode::Replace,
            CloneOptions {
                target_dimension: Some("minecraft:the_nether".into()),
                ..CloneOptions::default()
            },
        )
        .unwrap_err();
        assert!(
            error.contains("between dimensions needs the world loop"),
            "{error}"
        );
        assert_eq!(world.get(8, 64, 0), 0);
        // The command's own dimension is accepted.
        clone_with(
            &world,
            (0.0, 64.0, 0.0),
            (0.0, 64.0, 0.0),
            (8.0, 64.0, 0.0),
            CloneMode::Replace,
            CloneOptions {
                source_dimension: Some("minecraft:overworld".into()),
                target_dimension: Some("minecraft:overworld".into()),
                ..CloneOptions::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn clone_and_fill_trees_follow_commands_json() {
        let tree = super::super::commands::dispatcher();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let operator = source(2, tx);
        let mut seen = |input: &str| -> WorldCommand {
            tree.execute(input, operator.clone())
                .unwrap_or_else(|e| panic!("{input}: {e:?}"));
            match rx.try_recv().unwrap() {
                Action::World(command) => command,
                other => panic!("{input}: {other:?}"),
            }
        };
        let WorldCommand::Clone { mode, options, .. } =
            seen("clone 0 64 0 1 64 1 4 64 4 filtered #minecraft:logs[axis=y] move")
        else {
            panic!("clone");
        };
        assert_eq!(mode, CloneMode::Filtered);
        assert_eq!(options.mover, CloneMove::Move);
        assert!(options.filter.is_some());
        let WorldCommand::Clone { options, .. } = seen(
            "clone from minecraft:overworld 0 64 0 1 64 1 to minecraft:overworld 4 64 4 strict masked force",
        ) else {
            panic!("clone");
        };
        assert!(options.strict);
        assert_eq!(options.mover, CloneMove::Force);
        assert_eq!(
            options.source_dimension.as_deref(),
            Some("minecraft:overworld")
        );
        assert_eq!(
            options.target_dimension.as_deref(),
            Some("minecraft:overworld")
        );
        seen("clone 0 64 0 1 64 1 to minecraft:the_end 4 64 4");
        // fill: the filter follows `replace`, with a trailing mode.
        let WorldCommand::Fill { mode, filter, .. } =
            seen("fill 0 64 0 2 64 2 glass replace #minecraft:logs hollow")
        else {
            panic!("fill");
        };
        assert_eq!(mode, FillMode::Hollow);
        assert!(filter.is_some());
        let WorldCommand::Fill { mode, filter, .. } = seen("fill 0 64 0 2 64 2 glass replace")
        else {
            panic!("fill");
        };
        assert_eq!((mode, filter.is_some()), (FillMode::Replace, false));
        seen("fill 0 64 0 2 64 2 glass strict");
        seen("setblock 1 2 3 stone strict");
        // The old non-vanilla `<mode> <filter>` forms are gone.
        assert!(
            tree.execute("fill 0 64 0 2 64 2 glass destroy stone", operator.clone())
                .is_err()
        );
        assert!(
            tree.execute("fill 0 64 0 2 64 2 glass replace #minecraft:nope", operator)
                .is_err(),
            "unknown block tag"
        );
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

    #[test]
    fn template_entities_spawn_at_the_origin_plus_their_offset() {
        let mut nbt = ferrite_nbt::Compound::new();
        nbt.insert(
            "id".into(),
            ferrite_nbt::Tag::String("minecraft:pig".into()),
        );
        let mut unknown = ferrite_nbt::Compound::new();
        unknown.insert(
            "id".into(),
            ferrite_nbt::Tag::String("minecraft:nope".into()),
        );
        let command = WorldCommand::Template {
            pos: pos(10.0, 64.0, 10.0),
            name: "t".into(),
            blocks: std::sync::Arc::default(),
            entities: std::sync::Arc::default(),
            spawns: std::sync::Arc::new(vec![
                super::super::structure_template::TemplateEntity {
                    pos: [1.5, 0.0, 2.25],
                    nbt,
                },
                super::super::structure_template::TemplateEntity {
                    pos: [0.0; 3],
                    nbt: unknown,
                },
            ]),
            strict: false,
        };
        let spawned = template_spawns(&command, &origin());
        assert_eq!(spawned.len(), 1, "the unknown type is dropped");
        assert_eq!(spawned[0].0, [11.5, 64.0, 12.25]);
        let kind = ferrite_data::entity_props::by_name("minecraft:pig")
            .unwrap()
            .id;
        assert_eq!(spawned[0].1.data.kind, kind);
        assert!(
            template_spawns(
                &WorldCommand::SetBlock {
                    pos: pos(0.0, 0.0, 0.0),
                    block: 0,
                    mode: SetMode::Replace,
                    nbt: None
                },
                &origin()
            )
            .is_empty()
        );
    }

    fn template_of(blocks: Vec<([i32; 3], u16)>, strict: bool) -> WorldCommand {
        WorldCommand::Template {
            pos: pos(10.0, 64.0, 10.0),
            name: "t".into(),
            blocks: std::sync::Arc::new(blocks),
            entities: std::sync::Arc::default(),
            spawns: std::sync::Arc::default(),
            strict,
        }
    }

    fn state(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).expect(name)
    }

    #[test]
    fn a_non_strict_template_drops_a_placed_block_that_cannot_survive() {
        // `StructureTemplate.placeInWorld` -> `updateShapeAtEdge` / `updateFromNeighbourShapes`:
        // a torch placed on nothing is replaced by air. A strict placement leaves it.
        let torch = state("minecraft:torch");
        let stone = state("minecraft:stone");
        let loose = vec![([0, 0, 0], torch)];
        let world = TestWorld::new();
        let outcome = execute(&template_of(loose.clone(), false), &origin(), &world).unwrap();
        assert_eq!(world.get(10, 64, 10), 0, "the unsupported torch is air");
        assert_eq!(outcome.changes.len(), 2, "the placement and its removal");
        let strict = TestWorld::new();
        execute(&template_of(loose, true), &origin(), &strict).unwrap();
        assert_eq!(strict.get(10, 64, 10), u32::from(torch));
        // On a stone from the same template the torch stays, whatever the order of the blocks.
        for blocks in [
            vec![([0, 0, 0], stone), ([0, 1, 0], torch)],
            vec![([0, 1, 0], torch), ([0, 0, 0], stone)],
        ] {
            let world = TestWorld::new();
            execute(&template_of(blocks, false), &origin(), &world).unwrap();
            assert_eq!(world.get(10, 65, 10), u32::from(torch));
        }
    }

    #[test]
    fn a_non_strict_template_also_updates_the_world_blocks_at_its_edge() {
        // A torch of the world stands on stone; the template replaces that stone with a slab that
        // gives no support from above... here: with air. The neighbour outside the template
        // loses its support, and only the edge face between them can tell.
        let torch = state("minecraft:torch");
        let stone = state("minecraft:stone");
        for (strict, survives) in [(false, false), (true, true)] {
            let world = TestWorld::new();
            world.put(10, 64, 10, stone);
            world.put(10, 65, 10, torch);
            execute(
                &template_of(vec![([0, 0, 0], 0)], strict),
                &origin(),
                &world,
            )
            .unwrap();
            assert_eq!(
                world.get(10, 65, 10) == u32::from(torch),
                survives,
                "{strict}"
            );
        }
        // Not touching the template: a torch two blocks above is left alone.
        let world = TestWorld::new();
        world.put(10, 66, 10, torch);
        execute(
            &template_of(vec![([0, 0, 0], state("minecraft:stone"))], false),
            &origin(),
            &world,
        )
        .unwrap();
        assert_eq!(world.get(10, 66, 10), u32::from(torch));
    }

    #[test]
    fn extracted_templates_keep_their_blocks_under_the_non_strict_fix_up() {
        // Vanilla's templates are built so that the fix-up removes (almost) nothing; a block this
        // server removes is a support rule that is stricter than vanilla's `updateShape`.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft/structure");
        let mut files = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "nbt") {
                    files.push(path);
                }
            }
        }
        files.sort();
        let mut removed_by: std::collections::BTreeMap<String, usize> = Default::default();
        let mut total = 0;
        // Every eighth template keeps the run short; all 1511 pass (checked by hand).
        for path in files.iter().step_by(8) {
            let relative = path.strip_prefix(&root).unwrap().with_extension("");
            let id = format!(
                "minecraft:{}",
                relative.to_string_lossy().replace('\\', "/")
            );
            let Some(template) = super::super::structure_template::Template::load(&id) else {
                continue;
            };
            let (blocks, _) = template.place_with_entities(
                super::super::structure_template::Rotation::None,
                super::super::structure_template::Mirror::None,
                1.0,
                1,
            );
            let world = TestWorld::new();
            let Ok(outcome) = execute(&template_of(blocks.clone(), false), &origin(), &world)
            else {
                continue;
            };
            total += 1;
            for change in &outcome.changes[blocks.len()..] {
                let at = BlockPos::unpack(change.location);
                let old = blocks
                    .iter()
                    .find(|([x, y, z], _)| (10 + x, 64 + y, 10 + z) == (at.x, at.y, at.z))
                    .map(|(_, state)| {
                        ferrite_data::blocks::state(*state).map_or("?", |(name, _)| name)
                    });
                *removed_by
                    .entry(format!("{:?} in {id}", old.unwrap_or("?")))
                    .or_default() += 1;
            }
        }
        // What is left is a block whose support lies outside its own template (a hanging lantern,
        // a door or flower at the edge, a wall torch): vanilla's `updateShape` removes those in
        // an empty world too, and jigsaw pieces avoid it with `knownShape`.
        let unexpected: Vec<_> = removed_by
            .keys()
            .filter(|key| {
                !["lantern\"", "door\"", "poppy\"", "wall_torch\""]
                    .iter()
                    .any(|name| key.split(" in ").next().unwrap_or("").ends_with(name))
            })
            .collect();
        assert!(
            unexpected.is_empty(),
            "{unexpected:?} over {total} templates"
        );
    }

    #[test]
    fn templates_place_their_block_entities_beside_the_blocks() {
        let chest = ferrite_data::blocks::default_state("minecraft:chest").unwrap();
        let mut data = ferrite_nbt::Compound::new();
        data.insert(
            "id".into(),
            ferrite_nbt::Tag::String("minecraft:chest".into()),
        );
        let world = TestWorld::new();
        execute(
            &WorldCommand::Template {
                pos: pos(10.0, 64.0, 10.0),
                name: "structure minecraft:igloo".into(),
                blocks: std::sync::Arc::new(vec![([1, 0, 2], chest)]),
                entities: std::sync::Arc::new(vec![([1, 0, 2], data)]),
                spawns: std::sync::Arc::default(),
                strict: false,
            },
            &origin(),
            &world,
        )
        .unwrap();
        let entities = world.entities.borrow();
        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].0,
            [11, 64, 12],
            "entity positions follow the template origin"
        );
    }

    mod placed_block_entities {
        use super::*;
        use crate::game::anvil::AnvilChunkSource;

        fn loaded_source() -> std::sync::Arc<AnvilChunkSource> {
            let source = AnvilChunkSource::new_memory();
            assert!(source.ensure_chunk_loaded(0, 0, std::time::Duration::from_secs(10)));
            source
        }

        fn block_state(name: &str) -> u16 {
            ferrite_data::blocks::default_state(name).expect(name)
        }

        fn set_block(
            source: &AnvilChunkSource,
            name: &str,
            at: [i32; 3],
            tag: Option<&str>,
        ) -> Result<Outcome, String> {
            execute(
                &WorldCommand::SetBlock {
                    pos: pos(f64::from(at[0]), f64::from(at[1]), f64::from(at[2])),
                    block: block_state(name),
                    mode: SetMode::Replace,
                    nbt: tag.map(|text| ferrite_nbt::snbt::parse(text).unwrap()),
                },
                &origin(),
                source,
            )
        }

        #[tokio::test]
        async fn setblock_with_a_tag_creates_a_chest_furnace_and_sign_entity() {
            let source = loaded_source();
            let cases = [
                (
                    "minecraft:chest",
                    [2, 70, 2],
                    "{Items:[{Slot:0b,id:\"minecraft:stone\",count:3}]}",
                    "Items",
                ),
                (
                    "minecraft:furnace",
                    [4, 70, 2],
                    "{BurnTime:200s}",
                    "BurnTime",
                ),
                (
                    "minecraft:oak_sign",
                    [6, 70, 2],
                    "{is_waxed:1b}",
                    "is_waxed",
                ),
                (
                    "minecraft:oak_wall_hanging_sign",
                    [8, 70, 2],
                    "{is_waxed:1b}",
                    "is_waxed",
                ),
            ];
            for (block, at, tag, key) in cases {
                set_block(&source, block, at, Some(tag))
                    .unwrap_or_else(|error| panic!("{block}: {error}"));
                let entity = source
                    .block_entity(at[0], at[1], at[2])
                    .unwrap_or_else(|| panic!("{block}: no block entity"));
                assert!(
                    entity.data.contains_key(key),
                    "{block} keeps its tag key {key}"
                );
                assert_eq!(entity.data.get("x"), Some(&Tag::Int(at[0])), "{block} x");
                assert_eq!(entity.data.get("z"), Some(&Tag::Int(at[2])), "{block} z");
            }
            let chest = source.block_entity(2, 70, 2).unwrap();
            assert_eq!(
                chest.data.get("id"),
                Some(&Tag::String("minecraft:chest".into()))
            );
            let sign = source.block_entity(6, 70, 2).unwrap();
            assert_eq!(
                sign.data.get("id"),
                Some(&Tag::String("minecraft:sign".into()))
            );
            let hanging = source.block_entity(8, 70, 2).unwrap();
            assert_eq!(
                hanging.data.get("id"),
                Some(&Tag::String("minecraft:hanging_sign".into()))
            );
        }

        #[tokio::test]
        async fn placing_without_a_tag_creates_an_empty_entity_and_keeps_an_existing_one() {
            let source = loaded_source();
            let tracking = Tracking::default();
            let place = |name: &str| {
                let command = WorldCommand::SetBlock {
                    pos: pos(5.0, 71.0, 5.0),
                    block: block_state(name),
                    mode: SetMode::Replace,
                    nbt: None,
                };
                apply_world_result(&command, &origin(), &*source, &tracking).unwrap();
            };
            place("minecraft:smoker");
            let fresh = source
                .block_entity(5, 71, 5)
                .expect("a placed smoker gets its block entity");
            assert_eq!(
                fresh.data.get("id"),
                Some(&Tag::String("minecraft:smoker".into()))
            );
            // Give it contents; placing the same block again must not wipe them.
            source
                .update_block_entity(5, 71, 5, |slot| {
                    slot.as_mut()
                        .unwrap()
                        .data
                        .insert("BurnTime".into(), Tag::Short(99));
                    (true, ())
                })
                .unwrap();
            place("minecraft:smoker");
            assert_eq!(
                source.block_entity(5, 71, 5).unwrap().data.get("BurnTime"),
                Some(&Tag::Short(99))
            );
            // A block that carries no block entity gets none.
            let command = WorldCommand::SetBlock {
                pos: pos(7.0, 71.0, 5.0),
                block: block_state("minecraft:stone"),
                mode: SetMode::Replace,
                nbt: None,
            };
            apply_world_result(&command, &origin(), &*source, &tracking).unwrap();
            assert!(source.block_entity(7, 71, 5).is_none());
        }

        #[tokio::test]
        async fn a_tag_on_a_block_without_an_entity_is_still_rejected_and_changes_nothing() {
            let source = loaded_source();
            let before = source.block(9, 70, 9);
            let error =
                set_block(&source, "minecraft:stone", [9, 70, 9], Some("{Items:[]}")).unwrap_err();
            assert!(error.contains("not a block entity"), "{error}");
            assert_eq!(source.block(9, 70, 9), before, "the block was not placed");
            assert!(source.block_entity(9, 70, 9).is_none());
        }
    }
}
