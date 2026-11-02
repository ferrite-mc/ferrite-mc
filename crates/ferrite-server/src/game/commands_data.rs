//! `/data`: read and edit NBT on entities, block entities and world storage.
//!
//! Storage lives on [`GameState`]; player NBT is the [`PlayerEntry`] view the game loop
//! keeps. Block entities belong to a chunk, so those operations are handed to the game loop
//! as [`Action::Data`] and applied against the chunk source.

use super::commands::{Action, Feedback, GameState, PlayerEntry, Source, send};
use super::nbt_path::{NbtPath, append, prepend};
use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, argument, literal,
};
use ferrite_nbt::{Compound, List, Tag, snbt};

/// What to do to a block entity's NBT.
#[derive(Debug, Clone, PartialEq)]
pub enum ModifyOp {
    Set(Tag),
    Merge(Tag),
    Append(Tag),
    Prepend(Tag),
    Insert(usize, Tag),
}

/// A block-entity NBT operation, applied by the game loop.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockData {
    Get {
        pos: [i32; 3],
        path: String,
    },
    Merge {
        pos: [i32; 3],
        data: Tag,
    },
    Modify {
        pos: [i32; 3],
        path: String,
        op: ModifyOp,
    },
    Remove {
        pos: [i32; 3],
        path: String,
    },
}

/// Where a `/data` subcommand points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Entity,
    Block,
    Storage,
}

/// The NBT a player exposes to `/data`.
pub fn player_tag(player: &PlayerEntry) -> Tag {
    let mut map = Compound::new();
    let id = player.uuid.as_u128();
    map.insert(
        "UUID".to_owned(),
        Tag::IntArray(vec![
            (id >> 96) as i32,
            (id >> 64) as i32,
            (id >> 32) as i32,
            id as i32,
        ]),
    );
    map.insert("id".to_owned(), Tag::String(player.uuid.to_string()));
    map.insert(
        "Pos".to_owned(),
        Tag::List(List::Double(vec![
            player.pos[0],
            player.pos[1],
            player.pos[2],
        ])),
    );
    map.insert(
        "Rotation".to_owned(),
        Tag::List(List::Float(vec![player.yaw, player.pitch])),
    );
    map.insert(
        "Dimension".to_owned(),
        Tag::String("minecraft:overworld".into()),
    );
    map.insert("Health".to_owned(), Tag::Float(20.0));
    map.insert("OnGround".to_owned(), Tag::Byte(1));
    map.insert(
        "Tags".to_owned(),
        Tag::List(List::String(player.tags.iter().cloned().collect())),
    );
    Tag::Compound(map)
}

/// Writes the parts of a player's NBT view that the server can honour back into `player`.
pub fn apply_player_tag(player: &mut PlayerEntry, tag: &Tag) {
    let Tag::Compound(map) = tag else {
        return;
    };
    if let Some(Tag::List(List::Double(pos))) = map.get("Pos")
        && pos.len() == 3
    {
        player.pos = [pos[0], pos[1], pos[2]];
    }
    if let Some(Tag::List(List::Float(rotation))) = map.get("Rotation")
        && rotation.len() == 2
    {
        player.yaw = rotation[0];
        player.pitch = rotation[1];
    }
    if let Some(Tag::List(List::String(tags))) = map.get("Tags") {
        player.tags = tags.iter().cloned().collect();
    }
}

fn position(ctx: &CommandContext<Source>) -> Result<[i32; 3], CommandError> {
    match ctx.argument("pos") {
        Some(ferrite_command::ArgumentValue::BlockPos(coords)) => {
            let source = ctx.source();
            let pos = coords.resolve(source.origin, source.yaw, source.pitch);
            Ok([
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32,
            ])
        }
        _ => Err(CommandError::failed("Missing block position")),
    }
}

fn path_arg(ctx: &CommandContext<Source>, name: &str) -> Result<NbtPath, CommandError> {
    NbtPath::parse(ctx.string(name).unwrap_or("")).map_err(CommandError::failed)
}

fn nbt_arg(ctx: &CommandContext<Source>, name: &str) -> Result<Tag, CommandError> {
    snbt::parse(ctx.string(name).unwrap_or_default())
        .map_err(|error| CommandError::failed(error.to_string()))
}

pub(crate) fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn storage_name(ctx: &CommandContext<Source>) -> Result<String, CommandError> {
    let raw = ctx.string("target").unwrap_or_default();
    let id = namespaced(raw);
    let mut parts = id.split(':');
    if parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_some()
    {
        return Err(CommandError::failed(format!("Invalid storage id '{raw}'")));
    }
    Ok(id)
}

/// Resolves an entity argument against the online players.
fn target_players(ctx: &CommandContext<Source>, name: &str) -> Result<uuid::Uuid, CommandError> {
    let target = match ctx.argument(name) {
        Some(ferrite_command::ArgumentValue::Entity(target)) => target.clone(),
        _ => return Err(CommandError::failed("Missing entity target")),
    };
    let source = ctx.source();
    let state = source.state.lock().unwrap();
    let mut players: Vec<PlayerEntry> = match &target {
        ferrite_command::arguments::EntityTarget::Name(wanted) => state
            .players
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(wanted))
            .cloned()
            .collect(),
        ferrite_command::arguments::EntityTarget::Uuid(raw) => uuid::Uuid::parse_str(raw)
            .ok()
            .map(|uuid| {
                state
                    .players
                    .iter()
                    .filter(|p| p.uuid == uuid)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        ferrite_command::arguments::EntityTarget::Selector { kind, .. } => match kind {
            ferrite_command::arguments::SelectorKind::Myself => state
                .players
                .iter()
                .filter(|p| Some(p.uuid) == source.uuid)
                .cloned()
                .collect(),
            _ => state.players.clone(),
        },
    };
    players.sort_by(|a, b| a.name.cmp(&b.name));
    players.dedup_by_key(|p| p.uuid);
    players.first().map(|player| player.uuid).ok_or_else(|| {
        CommandError::failed(super::feedback::render(
            "argument.entity.notfound.player",
            &[],
        ))
    })
}

fn missing(path: &NbtPath) -> CommandError {
    CommandError::failed(super::feedback::render(
        "commands.data.get.unknown",
        &[path.text()],
    ))
}

fn storage_slot<'a>(state: &'a mut GameState, id: &str) -> &'a mut Tag {
    state
        .storage
        .entry(id.to_owned())
        .or_insert_with(|| Tag::Compound(Compound::new()))
}

fn modified(target: Target, args: Vec<String>) -> Feedback {
    let key = match target {
        Target::Storage => "commands.data.storage.modified",
        Target::Entity => "commands.data.entity.modified",
        Target::Block => "commands.data.block.modified",
    };
    Feedback::new(key, args)
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    let root = literal::<Source>("data")
        .requires(|source| source.permission >= 2)
        .restricted(true);
    let data = tree.register(root);
    for target in [Target::Entity, Target::Block, Target::Storage] {
        for branch in [
            get_branch(target),
            merge_branch(target),
            modify_branch(target),
            remove_branch(target),
        ] {
            tree.add_child(data, branch);
        }
    }
}

fn target_name(target: Target) -> &'static str {
    match target {
        Target::Entity => "entity",
        Target::Block => "block",
        Target::Storage => "storage",
    }
}

fn target_node(target: Target) -> ferrite_command::NodeBuilder<Source> {
    match target {
        Target::Entity => argument::<Source>("target", ArgumentType::entities()),
        Target::Block => argument::<Source>("pos", ArgumentType::BlockPos),
        Target::Storage => argument::<Source>("target", ArgumentType::word()),
    }
}

fn path_node() -> ferrite_command::NodeBuilder<Source> {
    argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
}

fn get_branch(target: Target) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("get").then(
        literal::<Source>(target_name(target)).then(
            target_node(target)
                .executes(move |ctx| get(ctx, target))
                .then(path_node().executes(move |ctx| get(ctx, target))),
        ),
    )
}

fn merge_branch(target: Target) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("merge").then(
        literal::<Source>(target_name(target)).then(
            target_node(target).then(
                argument::<Source>("nbt", ArgumentType::opaque("minecraft:nbt_compound_tag"))
                    .executes(move |ctx| merge(ctx, target)),
            ),
        ),
    )
}

fn modify_branch(target: Target) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("modify").then(
        literal::<Source>(target_name(target))
            .then(target_node(target).then(path_node().also(operations(target)))),
    )
}

/// The mutation kinds, as siblings under the path node.
fn operations(target: Target) -> Vec<ferrite_command::NodeBuilder<Source>> {
    let mut out = vec![
        literal::<Source>("set")
            .then(literal::<Source>("value").then(
                argument::<Source>("value", ArgumentType::opaque("minecraft:nbt_tag")).executes(
                    move |ctx| modify(ctx, target, |_| Ok(ModifyOp::Set(nbt_arg(ctx, "value")?))),
                ),
            ))
            .then(literal::<Source>("from").then(from_node(target, ModifyOp::Set))),
    ];
    for (name, make) in [
        ("merge", ModifyOp::Merge as fn(Tag) -> ModifyOp),
        ("append", ModifyOp::Append),
        ("prepend", ModifyOp::Prepend),
    ] {
        out.push(
            literal::<Source>(name)
                .then(
                    literal::<Source>("value").then(
                        argument::<Source>("value", ArgumentType::opaque("minecraft:nbt_tag"))
                            .executes(move |ctx| {
                                modify(ctx, target, move |_| Ok(make(nbt_arg(ctx, "value")?)))
                            }),
                    ),
                )
                .then(literal::<Source>("from").then(from_node(target, make))),
        );
    }
    out.push(literal::<Source>("insert").then(
        argument::<Source>("index", ArgumentType::integer_range(0, i32::MAX)).then(
            literal::<Source>("value").then(
                argument::<Source>("value", ArgumentType::opaque("minecraft:nbt_tag")).executes(
                    move |ctx| {
                        let index = ctx.integer("index").unwrap_or(0) as usize;
                        modify(ctx, target, move |_| {
                            Ok(ModifyOp::Insert(index, nbt_arg(ctx, "value")?))
                        })
                    },
                ),
            ),
        ),
    ));
    out
}

/// `from entity|storage <source> [path]`, resolving the source value immediately.
fn from_node(target: Target, make: fn(Tag) -> ModifyOp) -> ferrite_command::NodeBuilder<Source> {
    let entity = literal::<Source>("entity").then(
        argument::<Source>("source", ArgumentType::entities())
            .executes(move |ctx| modify_from(ctx, target, make, SourceKind::Entity))
            .then(
                argument::<Source>("sourcePath", ArgumentType::opaque("minecraft:nbt_path"))
                    .executes(move |ctx| modify_from(ctx, target, make, SourceKind::Entity)),
            ),
    );
    let storage = literal::<Source>("storage").then(
        argument::<Source>("source", ArgumentType::word())
            .executes(move |ctx| modify_from(ctx, target, make, SourceKind::Storage))
            .then(
                argument::<Source>("sourcePath", ArgumentType::opaque("minecraft:nbt_path"))
                    .executes(move |ctx| modify_from(ctx, target, make, SourceKind::Storage)),
            ),
    );
    entity.also([storage])
}

#[derive(Clone, Copy)]
enum SourceKind {
    Entity,
    Storage,
}

fn remove_branch(target: Target) -> ferrite_command::NodeBuilder<Source> {
    literal::<Source>("remove").then(
        literal::<Source>(target_name(target))
            .then(target_node(target).then(path_node().executes(move |ctx| remove(ctx, target)))),
    )
}

fn get(ctx: &CommandContext<Source>, target: Target) -> Result<i32, CommandError> {
    let path = path_arg(ctx, "path").unwrap_or_else(|_| NbtPath::parse("").unwrap());
    match target {
        Target::Storage => {
            let id = storage_name(ctx)?;
            let state = ctx.source().state.lock().unwrap();
            let Some(root) = state.storage.get(&id) else {
                return Err(missing(&path));
            };
            let value = if path.is_root() {
                root.clone()
            } else {
                path.get(root).ok_or_else(|| missing(&path))?
            };
            send(
                ctx.source(),
                Action::Feedback(Feedback::new(
                    "commands.data.storage.query",
                    vec![id, snbt::format(&value)],
                )),
            )
        }
        Target::Entity => {
            let uuid = target_players(ctx, "target")?;
            let state = ctx.source().state.lock().unwrap();
            let Some(player) = state.players.iter().find(|p| p.uuid == uuid) else {
                return Err(CommandError::failed("No player was found"));
            };
            let value = resolve_path(&player_tag(player), &path)?;
            send(
                ctx.source(),
                Action::Feedback(Feedback::new(
                    "commands.data.entity.query",
                    vec![player.name.clone(), snbt::format(&value)],
                )),
            )
        }
        Target::Block => {
            let pos = position(ctx)?;
            send(
                ctx.source(),
                Action::Data(BlockData::Get {
                    pos,
                    path: path.text().to_owned(),
                }),
            )
        }
    }
}

fn resolve_path(root: &Tag, path: &NbtPath) -> Result<Tag, CommandError> {
    if path.is_root() {
        return Ok(root.clone());
    }
    path.get(root).ok_or_else(|| missing(path))
}

fn merge(ctx: &CommandContext<Source>, target: Target) -> Result<i32, CommandError> {
    let data = nbt_arg(ctx, "nbt")?;
    match target {
        Target::Storage => {
            let id = storage_name(ctx)?;
            let mut state = ctx.source().state.lock().unwrap();
            let root = storage_slot(&mut state, &id);
            let changed = NbtPath::parse("")
                .unwrap()
                .merge(root, &data)
                .map_err(CommandError::failed)?;
            drop(state);
            if !changed {
                return Err(CommandError::failed(super::feedback::render(
                    "commands.data.merge.failed",
                    &[],
                )));
            }
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Storage, vec![id])),
            )
        }
        Target::Entity => {
            let uuid = target_players(ctx, "target")?;
            let mut state = ctx.source().state.lock().unwrap();
            let Some(index) = state.players.iter().position(|p| p.uuid == uuid) else {
                return Err(CommandError::failed("No player was found"));
            };
            let mut root = player_tag(&state.players[index]);
            let changed = NbtPath::parse("")
                .unwrap()
                .merge(&mut root, &data)
                .map_err(CommandError::failed)?;
            if !changed {
                return Err(CommandError::failed(super::feedback::render(
                    "commands.data.merge.failed",
                    &[],
                )));
            }
            let name = state.players[index].name.clone();
            apply_player_tag(&mut state.players[index], &root);
            drop(state);
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Entity, vec![name])),
            )
        }
        Target::Block => {
            let pos = position(ctx)?;
            send(ctx.source(), Action::Data(BlockData::Merge { pos, data }))
        }
    }
}

fn modify(
    ctx: &CommandContext<Source>,
    target: Target,
    make: impl Fn(&CommandContext<Source>) -> Result<ModifyOp, CommandError>,
) -> Result<i32, CommandError> {
    let op = make(ctx)?;
    let path = path_arg(ctx, "path")?;
    match target {
        Target::Storage => {
            let id = storage_name(ctx)?;
            let mut state = ctx.source().state.lock().unwrap();
            let root = storage_slot(&mut state, &id);
            apply_op(root, &path, &op)?;
            drop(state);
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Storage, vec![id])),
            )
        }
        Target::Entity => {
            let uuid = target_players(ctx, "target")?;
            let mut state = ctx.source().state.lock().unwrap();
            let Some(index) = state.players.iter().position(|p| p.uuid == uuid) else {
                return Err(CommandError::failed("No player was found"));
            };
            let mut root = player_tag(&state.players[index]);
            apply_op(&mut root, &path, &op)?;
            let name = state.players[index].name.clone();
            apply_player_tag(&mut state.players[index], &root);
            drop(state);
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Entity, vec![name])),
            )
        }
        Target::Block => {
            let pos = position(ctx)?;
            send(
                ctx.source(),
                Action::Data(BlockData::Modify {
                    pos,
                    path: path.text().to_owned(),
                    op,
                }),
            )
        }
    }
}

fn apply_op(root: &mut Tag, path: &NbtPath, op: &ModifyOp) -> Result<(), CommandError> {
    match op {
        ModifyOp::Set(value) | ModifyOp::Insert(_, value) => path.set(root, value.clone()),
        ModifyOp::Merge(value) => path.merge(root, value).map(|_| ()),
        ModifyOp::Append(value) => append(path, root, value.clone()),
        ModifyOp::Prepend(value) => prepend(path, root, value.clone()),
    }
    .map_err(CommandError::failed)
}

fn modify_from(
    ctx: &CommandContext<Source>,
    target: Target,
    make: fn(Tag) -> ModifyOp,
    kind: SourceKind,
) -> Result<i32, CommandError> {
    let value = read_source(ctx, kind)?;
    modify(ctx, target, move |_| Ok(make(value.clone())))
}

fn read_source(ctx: &CommandContext<Source>, kind: SourceKind) -> Result<Tag, CommandError> {
    let path = path_arg(ctx, "sourcePath").unwrap_or_else(|_| NbtPath::parse("").unwrap());
    match kind {
        SourceKind::Storage => {
            let id = namespaced(ctx.string("source").unwrap_or_default());
            let state = ctx.source().state.lock().unwrap();
            let Some(root) = state.storage.get(&id) else {
                return Err(missing(&path));
            };
            resolve_path(root, &path)
        }
        SourceKind::Entity => {
            let uuid = target_players(ctx, "source")?;
            let state = ctx.source().state.lock().unwrap();
            let Some(player) = state.players.iter().find(|p| p.uuid == uuid) else {
                return Err(CommandError::failed("No player was found"));
            };
            resolve_path(&player_tag(player), &path)
        }
    }
}

fn remove(ctx: &CommandContext<Source>, target: Target) -> Result<i32, CommandError> {
    let path = path_arg(ctx, "path")?;
    match target {
        Target::Storage => {
            let id = storage_name(ctx)?;
            let mut state = ctx.source().state.lock().unwrap();
            let root = storage_slot(&mut state, &id);
            path.remove(root).map_err(CommandError::failed)?;
            drop(state);
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Storage, vec![id])),
            )
        }
        Target::Entity => {
            let uuid = target_players(ctx, "target")?;
            let mut state = ctx.source().state.lock().unwrap();
            let Some(index) = state.players.iter().position(|p| p.uuid == uuid) else {
                return Err(CommandError::failed("No player was found"));
            };
            let mut root = player_tag(&state.players[index]);
            path.remove(&mut root).map_err(CommandError::failed)?;
            let name = state.players[index].name.clone();
            apply_player_tag(&mut state.players[index], &root);
            drop(state);
            send(
                ctx.source(),
                Action::Feedback(modified(Target::Entity, vec![name])),
            )
        }
        Target::Block => {
            let pos = position(ctx)?;
            send(
                ctx.source(),
                Action::Data(BlockData::Remove {
                    pos,
                    path: path.text().to_owned(),
                }),
            )
        }
    }
}

/// Applies a block-entity operation against `world`, returning the feedback to send.
pub fn apply_block(
    world: Option<&super::anvil::AnvilChunkSource>,
    command: &BlockData,
) -> Result<Feedback, String> {
    let pos = match command {
        BlockData::Get { pos, .. }
        | BlockData::Merge { pos, .. }
        | BlockData::Modify { pos, .. }
        | BlockData::Remove { pos, .. } => *pos,
    };
    let Some(world) = world else {
        return Err("World storage is unavailable".into());
    };
    let entity = world
        .block_entity(pos[0], pos[1], pos[2])
        .ok_or_else(|| "The target block is not a block entity".to_owned())?;
    let mut root = Tag::Compound(entity.data.clone());
    let coords = vec![pos[0].to_string(), pos[1].to_string(), pos[2].to_string()];
    match command {
        BlockData::Get { path, .. } => {
            let path = NbtPath::parse(path)?;
            let value = resolve_path(&root, &path).map_err(|error| error.to_string())?;
            Ok(if path.is_root() {
                Feedback::new(
                    "commands.data.block.query",
                    vec![coords.join(", "), snbt::format(&value)],
                )
            } else {
                Feedback::new(
                    "commands.data.block.get",
                    vec![
                        path.text().to_owned(),
                        coords.join(", "),
                        "1.0".into(),
                        snbt::format(&value),
                    ],
                )
            })
        }
        BlockData::Merge { data, .. } => {
            let path = NbtPath::parse("")?;
            if !path.merge(&mut root, data)? {
                return Err(
                    "Nothing changed. The specified properties already have these values".into(),
                );
            }
            write_block(world, pos, &root)?;
            Ok(Feedback::new("commands.data.block.modified", coords))
        }
        BlockData::Modify { path, op, .. } => {
            let path = NbtPath::parse(path)?;
            match op {
                ModifyOp::Set(value) | ModifyOp::Insert(_, value) => {
                    path.set(&mut root, value.clone())?
                }
                ModifyOp::Merge(value) => {
                    path.merge(&mut root, value)?;
                }
                ModifyOp::Append(value) => append(&path, &mut root, value.clone())?,
                ModifyOp::Prepend(value) => prepend(&path, &mut root, value.clone())?,
            }
            write_block(world, pos, &root)?;
            Ok(Feedback::new("commands.data.block.modified", coords))
        }
        BlockData::Remove { path, .. } => {
            let path = NbtPath::parse(path)?;
            path.remove(&mut root)?;
            write_block(world, pos, &root)?;
            Ok(Feedback::new("commands.data.block.modified", coords))
        }
    }
}

fn write_block(
    world: &super::anvil::AnvilChunkSource,
    pos: [i32; 3],
    root: &Tag,
) -> Result<(), String> {
    let Tag::Compound(data) = root else {
        return Err("Expected compound tag".into());
    };
    world
        .update_block_entity(pos[0], pos[1], pos[2], |entity| match entity {
            Some(entity) => {
                entity.data = data.clone();
                (true, ())
            }
            None => (false, ()),
        })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::commands::{Action, GameState};
    use super::*;
    use std::sync::{Arc, Mutex};

    fn source() -> (Source, tokio::sync::mpsc::UnboundedReceiver<Action>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(GameState::new(1, 0, 20)));
        state
            .lock()
            .unwrap()
            .join(uuid::Uuid::nil(), "Alice".into(), 1, 20);
        (
            Source {
                permission: 2,
                origin: [0.0, 64.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: Some(uuid::Uuid::nil()),
                name: Some("Alice".into()),
                anchor: ferrite_command::EntityAnchor::Feet,
                dimension: crate::game::Dimension::Overworld,
                world: None,
                tree: None,
                function_depth: 0,
                state,
                actions: tx,
            },
            rx,
        )
    }

    #[test]
    fn storage_round_trips_through_get_merge_modify_and_remove() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        tree.execute("data merge storage demo {a:{b:1},l:[1,2]}", source.clone())
            .unwrap();
        tree.execute("data get storage demo a.b", source.clone())
            .unwrap();
        tree.execute("data modify storage demo a.b set value 7", source.clone())
            .unwrap();
        tree.execute("data modify storage demo l append value 3", source.clone())
            .unwrap();
        tree.execute("data remove storage demo a.b", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        let texts: Vec<&str> = actions
            .iter()
            .filter_map(|action| match action {
                Action::Feedback(feedback) => Some(feedback.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts
                .iter()
                .any(|text| text.contains("has the following contents"))
        );
        let state = source.state.lock().unwrap();
        let root = state.storage.get("minecraft:demo").unwrap();
        assert_eq!(snbt::format(root), "{a:{},l:[1,2,3]}");
        drop(state);
        let error = tree
            .execute("data get storage demo missing", source.clone())
            .unwrap_err();
        assert!(error.to_string().contains("tag doesn't exist"));
        let error = tree
            .execute("data merge storage demo {a:{}}", source)
            .unwrap_err();
        assert!(error.to_string().contains("Nothing changed"));
    }

    #[test]
    fn entity_data_reads_the_player_view() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        tree.execute("data get entity @s Pos", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Feedback(feedback) if feedback.text.contains("Alice")
        )));
        tree.execute(
            "data modify entity @s Tags append value \"op\"",
            source.clone(),
        )
        .unwrap();
        assert!(source.state.lock().unwrap().players[0].tags.contains("op"));
        let error = tree.execute("data get entity @s Nope", source).unwrap_err();
        assert!(error.to_string().contains("tag doesn't exist"));
    }

    #[test]
    fn block_operations_are_emitted_as_actions() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        tree.execute("data get block 1 2 3 Items", source.clone())
            .unwrap();
        tree.execute("data merge block 1 2 3 {a:1}", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(matches!(
            actions[0],
            Action::Data(BlockData::Get { pos: [1, 2, 3], .. })
        ));
        assert!(matches!(
            actions[1],
            Action::Data(BlockData::Merge { pos: [1, 2, 3], .. })
        ));
        let error = tree.execute("data get block 1 2", source).unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
