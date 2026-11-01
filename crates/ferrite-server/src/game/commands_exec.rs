//! `/execute`: source modifiers, conditions and `store`.
//!
//! Brigadier implements `execute` as a chain of nodes that redirect back to `execute`, so a
//! command like `execute as @a at @s run say hi` re-parses the rest of the tree once per
//! source. This module does the same: each modifier rewrites the [`Source`] and forwards,
//! `if`/`unless` forward only when the test passes, and `store` attaches a result callback
//! ([`NodeBuilder::stores`]) that writes the inner command's result.

use super::commands::{Action, Source, send};
use super::commands_data::{BlockData, ModifyOp, apply_player_tag, namespaced, player_tag};
use super::nbt_path::NbtPath;
use ferrite_command::arguments::{EntityTarget, SelectorKind};
use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, EntityAnchor,
    NodeBuilder, NodeId, argument, literal,
};
use ferrite_nbt::Tag;
use uuid::Uuid;

/// A player resolved from an entity argument, with the state `execute` needs.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityView {
    pub uuid: Uuid,
    pub name: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// Resolves an entity argument against the online players (the only entities with a
/// command-visible position today).
pub fn resolve(
    state: &super::commands::GameState,
    target: &EntityTarget,
    source: Option<Uuid>,
    origin: [f64; 3],
) -> Result<Vec<EntityView>, CommandError> {
    use super::commands::PlayerEntry;
    let view = |player: &PlayerEntry| EntityView {
        uuid: player.uuid,
        name: player.name.clone(),
        pos: player.pos,
        yaw: player.yaw,
        pitch: player.pitch,
    };
    let mut entities: Vec<EntityView> = match target {
        EntityTarget::Name(name) => state
            .players
            .iter()
            .filter(|player| player.name.eq_ignore_ascii_case(name))
            .map(view)
            .collect(),
        EntityTarget::Uuid(raw) => Uuid::parse_str(raw)
            .ok()
            .map(|uuid| {
                state
                    .players
                    .iter()
                    .filter(|player| player.uuid == uuid)
                    .map(view)
                    .collect()
            })
            .unwrap_or_default(),
        EntityTarget::Selector { kind, .. } => match kind {
            SelectorKind::Myself => state
                .players
                .iter()
                .filter(|player| Some(player.uuid) == source)
                .map(view)
                .collect(),
            SelectorKind::AllPlayers | SelectorKind::AllEntities => {
                state.players.iter().map(view).collect()
            }
            SelectorKind::NearestPlayer | SelectorKind::NearestEntity => state
                .players
                .iter()
                .min_by(|a, b| distance(a.pos, origin).total_cmp(&distance(b.pos, origin)))
                .map(view)
                .into_iter()
                .collect(),
            SelectorKind::RandomPlayer => {
                let mut players: Vec<&PlayerEntry> = state.players.iter().collect();
                players.sort_by(|a, b| a.name.cmp(&b.name));
                if players.is_empty() {
                    Vec::new()
                } else {
                    let index = (Uuid::new_v4().as_u128() % players.len() as u128) as usize;
                    vec![view(players[index])]
                }
            }
        },
    };
    entities.sort_by(|a, b| a.name.cmp(&b.name));
    entities.dedup_by_key(|entity| entity.uuid);
    if let EntityTarget::Selector { options, .. } = target {
        for option in options {
            match option.key.as_str() {
                "name" => {
                    let wanted = option.value.trim_matches('"');
                    entities.retain(|entity| entity.name == wanted);
                }
                "type" => {
                    if option.value.trim_start_matches("minecraft:") != "player" {
                        entities.clear();
                    }
                }
                "limit" => {
                    if let Ok(limit) = option.value.parse::<usize>() {
                        entities.truncate(limit);
                    }
                }
                "sort" => match option.value.as_str() {
                    "nearest" => entities.sort_by(|a, b| {
                        distance(a.pos, origin).total_cmp(&distance(b.pos, origin))
                    }),
                    "furthest" => entities.sort_by(|a, b| {
                        distance(b.pos, origin).total_cmp(&distance(a.pos, origin))
                    }),
                    _ => {}
                },
                _ => {}
            }
        }
    }
    if entities.is_empty() {
        let key = if matches!(target, EntityTarget::Selector { .. }) {
            "argument.entity.notfound.entity"
        } else {
            "argument.entity.notfound.player"
        };
        return Err(CommandError::failed(super::feedback::render(key, &[])));
    }
    Ok(entities)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        .sqrt()
}

fn resolve_in(ctx: &CommandContext<Source>, name: &str) -> Result<Vec<EntityView>, CommandError> {
    let target = match ctx.argument(name) {
        Some(ArgumentValue::Entity(target)) => target.clone(),
        _ => return Err(CommandError::failed("Missing entity target")),
    };
    let source = ctx.source();
    let state = source.state.lock().unwrap();
    resolve(&state, &target, source.uuid, source.origin)
}

fn coordinates(ctx: &CommandContext<Source>, name: &str) -> Result<[f64; 3], CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::Vec3(coords)) | Some(ArgumentValue::BlockPos(coords)) => {
            let source = ctx.source();
            Ok(coords.resolve(source.origin, source.yaw, source.pitch))
        }
        _ => Err(CommandError::failed(format!("Missing position '{name}'"))),
    }
}

fn block_pos(ctx: &CommandContext<Source>, name: &str) -> Result<[i32; 3], CommandError> {
    let pos = coordinates(ctx, name)?;
    Ok([
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    ])
}

fn look_angles(from: [f64; 3], to: [f64; 3]) -> (f32, f32) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = if dx == 0.0 && dz == 0.0 {
        0.0
    } else {
        (dz.atan2(dx).to_degrees() - 90.0) as f32
    };
    let pitch = (-dy.atan2(horizontal).to_degrees()) as f32;
    (yaw, pitch)
}

/// The source position the anchor applies to (`anchored eyes` lifts it by the eye height).
fn anchored(source: &Source) -> [f64; 3] {
    let mut pos = source.origin;
    pos[1] += source.anchor.offset();
    pos
}

fn with_entity(source: &Source, entity: &EntityView) -> Source {
    let mut next = source.clone();
    next.uuid = Some(entity.uuid);
    next.name = Some(entity.name.clone());
    next
}

fn with_at(source: &Source, entity: &EntityView) -> Source {
    let mut next = with_entity(source, entity);
    next.origin = entity.pos;
    next.yaw = entity.yaw;
    next.pitch = entity.pitch;
    next
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    let root = tree.root();
    let execute = tree.register(
        literal::<Source>("execute")
            .requires(|source| source.permission >= 2)
            .restricted(true),
    );
    for branch in [
        literal::<Source>("run").redirect(root),
        as_branch(execute),
        at_branch(execute),
        positioned_branch(execute),
        rotated_branch(execute),
        facing_branch(execute),
        align_branch(execute),
        anchored_branch(execute),
        in_branch(execute),
    ] {
        tree.add_child(execute, branch);
    }
    for (name, unless) in [("if", false), ("unless", true)] {
        tree.add_child(execute, literal::<Source>(name));
        let node = tree.find_node(&["execute", name]).expect("condition node");
        for condition in conditions(execute, unless) {
            tree.add_child(node, condition);
        }
    }
    tree.add_child(execute, literal::<Source>("store"));
    let store = tree.find_node(&["execute", "store"]).expect("store node");
    for (mode, success) in [("result", false), ("success", true)] {
        tree.add_child(store, literal::<Source>(mode));
        let node = tree
            .find_node(&["execute", "store", mode])
            .expect("store mode");
        for target in store_targets(execute, success) {
            tree.add_child(node, target);
        }
    }
}

fn as_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("as").then(
        argument::<Source>("targets", ArgumentType::entities()).fork(execute, |ctx| {
            Ok(resolve_in(ctx, "targets")?
                .iter()
                .map(|entity| with_entity(ctx.source(), entity))
                .collect())
        }),
    )
}

fn at_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("at").then(
        argument::<Source>("targets", ArgumentType::entities()).fork(execute, |ctx| {
            Ok(resolve_in(ctx, "targets")?
                .iter()
                .map(|entity| with_at(ctx.source(), entity))
                .collect())
        }),
    )
}

fn positioned_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("positioned")
        .then(
            argument::<Source>("pos", ArgumentType::Vec3).redirect_with(execute, |ctx| {
                let mut source = ctx.source().clone();
                source.origin = coordinates(ctx, "pos")?;
                Ok(source)
            }),
        )
        .then(literal::<Source>("as").then(
            argument::<Source>("targets", ArgumentType::entities()).fork(execute, |ctx| {
                let origin = ctx.source().origin;
                Ok(resolve_in(ctx, "targets")?
                    .into_iter()
                    .map(|entity| {
                        let mut source = with_entity(ctx.source(), &entity);
                        source.origin = [entity.pos[0], origin[1], entity.pos[2]];
                        source
                    })
                    .collect())
            }),
        ))
}

fn rotated_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("rotated")
        .then(
            argument::<Source>("rotation", ArgumentType::Rotation).redirect_with(execute, |ctx| {
                let rotation = match ctx.argument("rotation") {
                    Some(ArgumentValue::Rotation(rotation)) => *rotation,
                    _ => return Err(CommandError::failed("Missing rotation")),
                };
                let mut source = ctx.source().clone();
                let (yaw, pitch) = ferrite_command::arguments::coords::resolve_rotation(
                    rotation,
                    source.yaw,
                    source.pitch,
                );
                source.yaw = yaw;
                source.pitch = pitch;
                Ok(source)
            }),
        )
        .then(literal::<Source>("as").then(
            argument::<Source>("targets", ArgumentType::entities()).fork(execute, |ctx| {
                Ok(resolve_in(ctx, "targets")?
                    .into_iter()
                    .map(|entity| {
                        let mut source = with_entity(ctx.source(), &entity);
                        source.yaw = entity.yaw;
                        source.pitch = entity.pitch;
                        source
                    })
                    .collect())
            }),
        ))
}

fn look_at(ctx: &CommandContext<Source>, target: [f64; 3]) -> Result<Source, CommandError> {
    let mut source = ctx.source().clone();
    let (yaw, pitch) = look_angles(anchored(&source), target);
    source.yaw = yaw;
    source.pitch = pitch;
    Ok(source)
}

fn facing_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("facing")
        .then(
            argument::<Source>("pos", ArgumentType::Vec3).redirect_with(execute, |ctx| {
                let target = coordinates(ctx, "pos")?;
                look_at(ctx, target)
            }),
        )
        .then(literal::<Source>("entity").then(
            argument::<Source>("targets", ArgumentType::entities()).then(
                argument::<Source>("facingAnchor", ArgumentType::EntityAnchor).redirect_with(
                    execute,
                    |ctx| {
                        let anchor = ctx
                            .argument("facingAnchor")
                            .and_then(ArgumentValue::as_entity_anchor)
                            .unwrap_or(EntityAnchor::Feet);
                        let mut target = resolve_in(ctx, "targets")?[0].pos;
                        target[1] += anchor.offset();
                        look_at(ctx, target)
                    },
                ),
            ),
        ))
}

fn align_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("align").then(
        argument::<Source>("axes", ArgumentType::word()).redirect_with(execute, |ctx| {
            let axes = ctx.string("axes").unwrap_or_default();
            if axes.is_empty() || !axes.chars().all(|c| matches!(c, 'x' | 'y' | 'z')) {
                return Err(CommandError::failed(format!("Unknown axis '{axes}'")));
            }
            let mut source = ctx.source().clone();
            for axis in axes.chars() {
                let index = match axis {
                    'x' => 0,
                    'y' => 1,
                    _ => 2,
                };
                source.origin[index] = source.origin[index].floor();
            }
            Ok(source)
        }),
    )
}

fn anchored_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("anchored").then(
        argument::<Source>("anchor", ArgumentType::EntityAnchor).redirect_with(execute, |ctx| {
            let anchor = ctx
                .argument("anchor")
                .and_then(ArgumentValue::as_entity_anchor)
                .unwrap_or(EntityAnchor::Feet);
            let mut source = ctx.source().clone();
            source.anchor = anchor;
            Ok(source)
        }),
    )
}

const DIMENSIONS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

fn in_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("in").then(
        argument::<Source>("dimension", ArgumentType::Dimension).redirect_with(execute, |ctx| {
            let dimension = ctx.string("dimension").unwrap_or_default();
            if !DIMENSIONS.contains(&dimension) {
                return Err(CommandError::failed(format!(
                    "No such dimension: {dimension}"
                )));
            }
            let mut source = ctx.source().clone();
            source.dimension = super::Dimension::from_key(dimension).unwrap_or_default();
            Ok(source)
        }),
    )
}

fn passed(
    ctx: &CommandContext<Source>,
    unless: bool,
    result: bool,
) -> Result<Source, CommandError> {
    if result != unless {
        Ok(ctx.source().clone())
    } else {
        Err(CommandError::failed("Test failed"))
    }
}

fn conditions(execute: NodeId, unless: bool) -> Vec<NodeBuilder<Source>> {
    let block = literal::<Source>("block").then(
        argument::<Source>("pos", ArgumentType::BlockPos).then(
            argument::<Source>("block", ArgumentType::BlockState)
                .redirect_with(execute, move |ctx| condition_block(ctx, unless)),
        ),
    );
    let blocks = literal::<Source>("blocks").then(
        argument::<Source>("start", ArgumentType::BlockPos).then(
            argument::<Source>("end", ArgumentType::BlockPos).then(
                argument::<Source>("destination", ArgumentType::BlockPos)
                    .then(
                        literal::<Source>("all").redirect_with(execute, move |ctx| {
                            condition_blocks(ctx, unless, false)
                        }),
                    )
                    .then(
                        literal::<Source>("masked")
                            .redirect_with(execute, move |ctx| condition_blocks(ctx, unless, true)),
                    ),
            ),
        ),
    );
    let entity = literal::<Source>("entity").then(
        argument::<Source>("entities", ArgumentType::entities())
            .redirect_with(execute, move |ctx| condition_entity(ctx, unless)),
    );
    let score = literal::<Source>("score").then(
        argument::<Source>("target", ArgumentType::ScoreHolder { multiple: false }).then(
            argument::<Source>("targetObjective", ArgumentType::word()).then(
                argument::<Source>("operation", ArgumentType::opaque("minecraft:operation")).then(
                    argument::<Source>("source", ArgumentType::ScoreHolder { multiple: false })
                        .then(
                            argument::<Source>("sourceObjective", ArgumentType::word())
                                .redirect_with(execute, move |ctx| condition_score(ctx, unless)),
                        ),
                ),
            ),
        ),
    );
    let predicate = literal::<Source>("predicate").then(
        argument::<Source>("predicate", ArgumentType::ResourceLocation)
            .redirect_with(execute, move |ctx| condition_predicate(ctx, unless)),
    );
    let data_block = literal::<Source>("data").then(
        literal::<Source>("block").then(
            argument::<Source>("pos", ArgumentType::BlockPos).then(
                argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                    .redirect_with(execute, move |ctx| {
                        condition_data(ctx, unless, DataKind::Block)
                    }),
            ),
        ),
    );
    let data_entity = literal::<Source>("data").then(
        literal::<Source>("entity").then(
            argument::<Source>("target", ArgumentType::entities()).then(
                argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                    .redirect_with(execute, move |ctx| {
                        condition_data(ctx, unless, DataKind::Entity)
                    }),
            ),
        ),
    );
    let data_storage = literal::<Source>("data").then(
        literal::<Source>("storage").then(
            argument::<Source>("source", ArgumentType::word()).then(
                argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                    .redirect_with(execute, move |ctx| {
                        condition_data(ctx, unless, DataKind::Storage)
                    }),
            ),
        ),
    );
    vec![
        block,
        blocks,
        entity,
        score,
        predicate,
        data_block,
        data_entity,
        data_storage,
    ]
}

fn condition_block(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let Some(world) = &ctx.source().world else {
        return passed(ctx, unless, false);
    };
    let pos = block_pos(ctx, "pos")?;
    let wanted = match ctx.argument("block") {
        Some(ArgumentValue::BlockState(value)) => super::commands_world::resolve_block(value)?,
        _ => return Err(CommandError::failed("Missing block")),
    };
    let matches = world.block(pos[0], pos[1], pos[2]) == Some(u32::from(wanted));
    passed(ctx, unless, matches)
}

fn condition_blocks(
    ctx: &CommandContext<Source>,
    unless: bool,
    masked: bool,
) -> Result<Source, CommandError> {
    let Some(world) = &ctx.source().world else {
        return passed(ctx, unless, false);
    };
    let start = block_pos(ctx, "start")?;
    let end = block_pos(ctx, "end")?;
    let destination = block_pos(ctx, "destination")?;
    let volume = (end[0] - start[0] + 1).unsigned_abs() as u64
        * (end[1] - start[1] + 1).unsigned_abs() as u64
        * (end[2] - start[2] + 1).unsigned_abs() as u64;
    if volume > 32768 {
        return Err(CommandError::failed(format!(
            "Too many blocks in the specified area (maximum 32768, but specified {volume})"
        )));
    }
    let mut matches = true;
    for x in 0..=(end[0] - start[0]).abs() {
        for y in 0..=(end[1] - start[1]).abs() {
            for z in 0..=(end[2] - start[2]).abs() {
                let x = start[0] + x * (end[0] - start[0]).signum();
                let y = start[1] + y * (end[1] - start[1]).signum();
                let z = start[2] + z * (end[2] - start[2]).signum();
                let source_block = world.block(x, y, z);
                let target_block = world.block(
                    destination[0] + x - start[0],
                    destination[1] + y - start[1],
                    destination[2] + z - start[2],
                );
                if masked && target_block.is_none() {
                    continue;
                }
                if source_block != target_block {
                    matches = false;
                }
            }
        }
    }
    passed(ctx, unless, matches)
}

fn condition_entity(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    passed(ctx, unless, resolve_in(ctx, "entities").is_ok())
}

fn score_holder_name(ctx: &CommandContext<Source>, name: &str) -> String {
    match ctx.argument(name) {
        Some(ArgumentValue::ScoreHolder(ferrite_command::arguments::ScoreHolder::All)) => {
            "*".to_owned()
        }
        Some(ArgumentValue::ScoreHolder(ferrite_command::arguments::ScoreHolder::Target(
            EntityTarget::Name(name),
        ))) => name.clone(),
        Some(ArgumentValue::ScoreHolder(ferrite_command::arguments::ScoreHolder::Target(
            EntityTarget::Uuid(uuid),
        ))) => uuid.clone(),
        _ => String::new(),
    }
}

fn condition_score(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let holder = score_holder_name(ctx, "target");
    let objective = ctx.string("targetObjective").unwrap_or_default().to_owned();
    let source_holder = score_holder_name(ctx, "source");
    let source_objective = ctx.string("sourceObjective").unwrap_or_default().to_owned();
    let operation = ctx.string("operation").unwrap_or("=");
    let state = ctx.source().state.lock().unwrap();
    let left = state.scores.get(&(holder, objective)).copied();
    let right = state
        .scores
        .get(&(source_holder, source_objective))
        .copied();
    let result = match (left, right) {
        (Some(left), Some(right)) => match operation {
            "=" => left == right,
            "<" => left < right,
            "<=" => left <= right,
            ">" => left > right,
            ">=" => left >= right,
            _ => false,
        },
        _ => false,
    };
    passed(ctx, unless, result)
}

fn condition_predicate(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let _ = ctx.string("predicate");
    // No predicate registry is wired into the server yet, so every predicate evaluates false,
    // exactly like a datapack with no predicates loaded.
    passed(ctx, unless, false)
}

#[derive(Clone, Copy)]
enum DataKind {
    Block,
    Entity,
    Storage,
}

fn condition_data(
    ctx: &CommandContext<Source>,
    unless: bool,
    kind: DataKind,
) -> Result<Source, CommandError> {
    let path = NbtPath::parse(ctx.string("path").unwrap_or("")).map_err(CommandError::failed)?;
    let exists = match kind {
        DataKind::Storage => {
            let id = namespaced(ctx.string("source").unwrap_or(""));
            let state = ctx.source().state.lock().unwrap();
            state
                .storage
                .get(&id)
                .is_some_and(|root| path.is_root() || path.get(root).is_some())
        }
        DataKind::Entity => {
            let target = match ctx.argument("target") {
                Some(ArgumentValue::Entity(target)) => target.clone(),
                _ => return Err(CommandError::failed("Missing entity target")),
            };
            let source = ctx.source();
            let state = source.state.lock().unwrap();
            resolve(&state, &target, source.uuid, source.origin)
                .ok()
                .and_then(|entities| {
                    state
                        .players
                        .iter()
                        .find(|player| player.uuid == entities[0].uuid)
                        .map(player_tag)
                })
                .is_some_and(|root| path.is_root() || path.get(&root).is_some())
        }
        DataKind::Block => {
            let Some(world) = &ctx.source().world else {
                return passed(ctx, unless, false);
            };
            let pos = block_pos(ctx, "pos")?;
            world
                .block_entity(pos[0], pos[1], pos[2])
                .map(|entity| Tag::Compound(entity.data))
                .is_some_and(|root| path.is_root() || path.get(&root).is_some())
        }
    };
    passed(ctx, unless, exists)
}

fn store_targets(execute: NodeId, success: bool) -> Vec<NodeBuilder<Source>> {
    let block = literal::<Source>("block").then(
        argument::<Source>("pos", ArgumentType::BlockPos).then(
            argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                .then(store_type(execute, StoreKind::Block, success)),
        ),
    );
    let entity = literal::<Source>("entity").then(
        argument::<Source>("target", ArgumentType::entities()).then(
            argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                .then(store_type(execute, StoreKind::Entity, success)),
        ),
    );
    let storage = literal::<Source>("storage").then(
        argument::<Source>("source", ArgumentType::word()).then(
            argument::<Source>("path", ArgumentType::opaque("minecraft:nbt_path"))
                .then(store_type(execute, StoreKind::Storage, success)),
        ),
    );
    vec![block, entity, storage]
}

/// `storage <path> <type> <scale>`: the scale node forwards to `execute` and records the
/// inner result on its way out.
fn store_type(execute: NodeId, kind: StoreKind, success: bool) -> NodeBuilder<Source> {
    argument::<Source>("type", ArgumentType::word()).then(
        argument::<Source>("scale", ArgumentType::double())
            .redirect_with(execute, |ctx| Ok(ctx.source().clone()))
            .stores(move |ctx, result| write_store(ctx, kind, success, result)),
    )
}

#[derive(Clone, Copy)]
enum StoreKind {
    Block,
    Entity,
    Storage,
}

fn store_value(ctx: &CommandContext<Source>, ty: &str, success: bool, result: i32) -> Tag {
    let scale = ctx
        .argument("scale")
        .and_then(ArgumentValue::as_f64)
        .unwrap_or(1.0);
    let value = if success {
        f64::from(i32::from(result != 0))
    } else {
        f64::from(result)
    } * scale;
    match ty {
        "byte" => Tag::Byte(value as i8),
        "short" => Tag::Short(value as i16),
        "long" => Tag::Long(value as i64),
        "float" => Tag::Float(value as f32),
        "double" => Tag::Double(value),
        _ => Tag::Int(value as i32),
    }
}

fn write_store(
    ctx: &CommandContext<Source>,
    kind: StoreKind,
    success: bool,
    result: i32,
) -> Result<(), CommandError> {
    let ty = match ctx.string("type").unwrap_or("int") {
        ty @ ("byte" | "short" | "int" | "long" | "float" | "double") => ty,
        other => {
            return Err(CommandError::failed(format!(
                "Unknown number type '{other}'"
            )));
        }
    };
    let path = NbtPath::parse(ctx.string("path").unwrap_or("")).map_err(CommandError::failed)?;
    let value = store_value(ctx, ty, success, result);
    match kind {
        StoreKind::Block => {
            let pos = block_pos(ctx, "pos")?;
            send(
                ctx.source(),
                Action::Data(BlockData::Modify {
                    pos,
                    path: path.text().to_owned(),
                    op: ModifyOp::Set(value),
                }),
            )?;
        }
        StoreKind::Entity => {
            let entities = resolve_in(ctx, "target")?;
            let source = ctx.source();
            let mut state = source.state.lock().unwrap();
            let Some(index) = state
                .players
                .iter()
                .position(|player| player.uuid == entities[0].uuid)
            else {
                return Err(CommandError::failed("No player was found"));
            };
            let mut root = player_tag(&state.players[index]);
            path.set(&mut root, value).map_err(CommandError::failed)?;
            apply_player_tag(&mut state.players[index], &root);
        }
        StoreKind::Storage => {
            let id = namespaced(ctx.string("source").unwrap_or(""));
            let mut state = ctx.source().state.lock().unwrap();
            let root = state
                .storage
                .entry(id)
                .or_insert_with(|| Tag::Compound(Default::default()));
            path.set(root, value).map_err(CommandError::failed)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::commands::{Action, GameState};
    use super::*;
    use ferrite_nbt::snbt;
    use std::sync::{Arc, Mutex};

    fn source() -> (Source, tokio::sync::mpsc::UnboundedReceiver<Action>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(GameState::new(0, 0, 20)));
        state
            .lock()
            .unwrap()
            .join(Uuid::nil(), "Alice".into(), 0, 20);
        (
            Source {
                permission: 2,
                origin: [10.0, 64.0, 10.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: Some(Uuid::nil()),
                name: Some("Alice".into()),
                anchor: EntityAnchor::Feet,
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
    fn modifiers_rewrite_the_source() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        for command in [
            "execute positioned 1 2 3 run data merge storage t {x:1}",
            "execute align xyz run data merge storage t {y:1}",
            "execute anchored eyes run data merge storage t {z:1}",
            "execute rotated 90 10 run data merge storage t {r:1}",
            "execute facing 0 0 0 run data merge storage t {f:1}",
            "execute in minecraft:overworld run data merge storage t {d:1}",
        ] {
            tree.execute(command, source.clone()).unwrap();
        }
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert_eq!(
            actions
                .iter()
                .filter(|action| matches!(action, Action::Feedback(_)))
                .count(),
            6
        );
        let error = tree
            .execute(
                "execute align q run data merge storage t {x:1}",
                source.clone(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("Unknown axis"));
        let error = tree
            .execute(
                "execute in minecraft:moon run data merge storage t {x:1}",
                source,
            )
            .unwrap_err();
        assert!(error.to_string().contains("No such dimension"));
    }

    #[test]
    fn as_and_at_fork_over_players() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        tree.execute(
            "execute as @a run data modify storage t list append value \"a\"",
            source.clone(),
        )
        .unwrap();
        tree.execute(
            "execute at @s run data merge storage t {at:1}",
            source.clone(),
        )
        .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::Feedback(_)))
        );
        let state = source.state.lock().unwrap();
        assert_eq!(
            snbt::format(state.storage.get("minecraft:t").unwrap()),
            "{list:[\"a\"],at:1}"
        );
    }

    #[test]
    fn conditions_gate_the_run() {
        let tree = super::super::commands::dispatcher();
        let (source, _received) = source();
        source
            .state
            .lock()
            .unwrap()
            .scores
            .insert(("Alice".into(), "points".into()), 5);
        tree.execute(
            "execute if score Alice points = Alice points run data merge storage c {ok:1}",
            source.clone(),
        )
        .unwrap();
        tree.execute(
            "execute if data storage c ok run data merge storage c {nested:1}",
            source.clone(),
        )
        .unwrap();
        tree.execute(
            "execute unless entity @e[name=Nobody] run data merge storage c {no:1}",
            source.clone(),
        )
        .unwrap();
        let error = tree
            .execute(
                "execute if score Alice points > Alice points run data merge storage c {bad:1}",
                source.clone(),
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "Test failed");
        let error = tree
            .execute(
                "execute if entity @e[name=Nobody] run data merge storage c {bad:1}",
                source.clone(),
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "Test failed");
        let error = tree
            .execute(
                "execute if predicate minecraft:none run data merge storage c {bad:1}",
                source,
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "Test failed");
    }

    #[test]
    fn store_writes_the_inner_result() {
        let tree = super::super::commands::dispatcher();
        let (source, _received) = source();
        tree.execute(
            "execute store result storage results value int 2 run data merge storage t {a:1}",
            source.clone(),
        )
        .unwrap();
        tree.execute(
            "execute store success storage success value int 1 run data merge storage t {b:1}",
            source.clone(),
        )
        .unwrap();
        let state = source.state.lock().unwrap();
        assert_eq!(
            snbt::format(state.storage.get("minecraft:results").unwrap()),
            "{value:2}"
        );
        assert_eq!(
            snbt::format(state.storage.get("minecraft:success").unwrap()),
            "{value:1}"
        );
        drop(state);
        assert!(
            tree.execute(
                "execute store result storage x value int 1 run no_such_command",
                source,
            )
            .is_err()
        );
    }

    #[test]
    fn run_reaches_the_inner_command() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source();
        tree.execute("execute run say hello", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::Broadcast(_)))
        );
        let error = tree.execute("execute run", source).unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
