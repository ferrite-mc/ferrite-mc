//! The `/execute` branches beyond the basic modifiers: `if|unless biome|loaded|function|
//! stopwatch|items|slots`, `positioned over`, `on`, `summon` and `store ... bossbar`.
//!
//! What these cannot do on this server is listed in `docs/COMMANDS-STATUS.md`: `on origin` knows
//! only arrow/trident shooters, item throwers and bobber owners (the other relations come from the
//! snapshot and the ride table), `summon` hands the new entity to the rest of the chain through
//! a provisional snapshot entry, and `items|slots entity` reads only the slots the regions publish (not a
//! player inventory).

use ferrite_command::arguments::{EntityTarget, RegistryParser};
use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandError, NodeBuilder, NodeId, argument,
    literal,
};
use ferrite_world::HeightmapKind;

use super::commands::{Source, send};
use super::commands_exec::{Gate, passed, resolve_in_optional};

fn block_position(ctx: &CommandContext<Source>, name: &str) -> Result<[i32; 3], CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::BlockPos(coords)) => {
            let source = ctx.source();
            let at = coords.resolve(source.origin, source.yaw, source.pitch);
            Ok([
                at[0].floor() as i32,
                at[1].floor() as i32,
                at[2].floor() as i32,
            ])
        }
        _ => Err(CommandError::failed(format!("Missing position '{name}'"))),
    }
}

fn normalized(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

// ---------------------------------------------------------------------------------------------
// Conditions.
// ---------------------------------------------------------------------------------------------

pub(super) fn conditions(execute: NodeId, unless: bool) -> Vec<NodeBuilder<Source>> {
    let biome = literal::<Source>("biome").then(
        argument::<Source>("pos", ArgumentType::BlockPos).then(
            argument::<Source>(
                "biome",
                ArgumentType::Registry {
                    parser: RegistryParser::ResourceOrTag,
                    registry: "minecraft:worldgen/biome".into(),
                },
            )
            .gate(execute, unless, condition_biome),
        ),
    );
    let loaded = literal::<Source>("loaded").then(
        argument::<Source>("pos", ArgumentType::BlockPos).gate(execute, unless, condition_loaded),
    );
    let function = literal::<Source>("function").then(
        argument::<Source>("name", ArgumentType::opaque("minecraft:function")).gate(
            execute,
            unless,
            condition_function,
        ),
    );
    let stopwatch = literal::<Source>("stopwatch").then(
        argument::<Source>("id", ArgumentType::ResourceLocation).then(
            argument::<Source>("range", ArgumentType::opaque("minecraft:float_range")).gate(
                execute,
                unless,
                condition_stopwatch,
            ),
        ),
    );
    let slot_source = || ArgumentType::opaque("minecraft:slot_source");
    let items = literal::<Source>("items")
        .then(
            literal::<Source>("block").then(
                argument::<Source>("source", ArgumentType::BlockPos).then(
                    argument::<Source>("slots", slot_source()).then(
                        argument::<Source>(
                            "item_predicate",
                            ArgumentType::opaque("minecraft:item_predicate"),
                        )
                        .gate(execute, unless, |ctx, unless| {
                            condition_container(ctx, unless, true)
                        }),
                    ),
                ),
            ),
        )
        .then(
            literal::<Source>("entity").then(
                argument::<Source>("source", ArgumentType::entities()).then(
                    argument::<Source>("slots", slot_source()).then(
                        argument::<Source>(
                            "item_predicate",
                            ArgumentType::opaque("minecraft:item_predicate"),
                        )
                        .gate(execute, unless, |ctx, unless| {
                            condition_entity_inventory(ctx, unless, true)
                        }),
                    ),
                ),
            ),
        );
    let slots = literal::<Source>("slots")
        .then(literal::<Source>("block").then(
            argument::<Source>("source", ArgumentType::BlockPos).then(
                argument::<Source>("slots", slot_source()).gate(execute, unless, |ctx, unless| {
                    condition_container(ctx, unless, false)
                }),
            ),
        ))
        .then(literal::<Source>("entity").then(
            argument::<Source>("source", ArgumentType::entities()).then(
                argument::<Source>("slots", slot_source()).gate(execute, unless, |ctx, unless| {
                    condition_entity_inventory(ctx, unless, false)
                }),
            ),
        ));
    vec![biome, loaded, function, stopwatch, items, slots]
}

fn condition_biome(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let Some(world) = &ctx.source().world else {
        return passed(ctx, unless, false);
    };
    let pos = block_position(ctx, "pos")?;
    let Some(actual) = world.biome_name(pos[0], pos[1], pos[2]) else {
        return passed(ctx, unless, false);
    };
    let matches = match ctx.argument("biome") {
        Some(ArgumentValue::Resource { tag: true, id }) => {
            super::commands_server::biome_tag_members(id, 0).contains(&actual)
        }
        Some(ArgumentValue::Resource { tag: false, id }) => normalized(id) == actual,
        _ => match ctx.string("biome") {
            Some(raw) if raw.starts_with('#') => {
                super::commands_server::biome_tag_members(raw, 0).contains(&actual)
            }
            Some(raw) => normalized(raw) == actual,
            None => return Err(CommandError::failed("Missing biome")),
        },
    };
    passed(ctx, unless, matches)
}

fn condition_loaded(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let pos = block_position(ctx, "pos")?;
    let loaded = ctx
        .source()
        .world
        .as_ref()
        .is_some_and(|world| world.block(pos[0], pos[1], pos[2]).is_some());
    passed(ctx, unless, loaded)
}

/// `if function <name>`: runs the function (or tag) for the current source; the test passes
/// when it returns a non-zero value, and a function that fails does not pass.
fn condition_function(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let name = ctx
        .string("name")
        .ok_or_else(|| CommandError::failed("Missing function name"))?;
    let result = super::functions::run_command(ctx.source(), name, None);
    match result {
        Ok(value) => passed(ctx, unless, value != 0),
        Err(CommandError::Failed(_)) => passed(ctx, unless, false),
        Err(other) => Err(other),
    }
}

/// `a..b`, `..b`, `a..` or `a` as an inclusive range of seconds.
fn float_range(raw: &str) -> Option<(f64, f64)> {
    let (min, max) = match raw.split_once("..") {
        Some((min, max)) => (min, max),
        None => (raw, raw),
    };
    let parse = |text: &str, default: f64| {
        if text.is_empty() {
            Some(default)
        } else {
            text.parse::<f64>().ok()
        }
    };
    let (min, max) = (parse(min, f64::NEG_INFINITY)?, parse(max, f64::INFINITY)?);
    (min <= max).then_some((min, max))
}

fn condition_stopwatch(ctx: &CommandContext<Source>, unless: bool) -> Result<Source, CommandError> {
    let id = normalized(ctx.string("id").unwrap_or_default());
    let raw = ctx.string("range").unwrap_or_default();
    let (min, max) =
        float_range(raw).ok_or_else(|| CommandError::failed(format!("Invalid range '{raw}'")))?;
    let elapsed = {
        let state = ctx.source().state.lock().unwrap();
        let Some(start) = state.more.stopwatches.get(&id).copied() else {
            return Err(CommandError::failed(format!(
                "Stopwatch '{id}' does not exist"
            )));
        };
        (state.game_time - start) as f64 / 20.0
    };
    passed(ctx, unless, (min..=max).contains(&elapsed))
}

/// An item predicate without component conditions: `*`, an item id or an `#item_tag`.
fn item_matches(raw: &str, item: u16) -> Result<bool, CommandError> {
    let raw = raw.trim();
    if raw.contains('[') || raw.contains('{') {
        return Err(CommandError::failed(
            "Item predicates with component conditions are not supported",
        ));
    }
    let name = normalized(ferrite_item::item_name(item).unwrap_or_default());
    if raw == "*" {
        return Ok(true);
    }
    if let Some(tag) = raw.strip_prefix('#') {
        let members = super::block_tags::members_of("item", tag).ok_or_else(|| {
            CommandError::failed(format!("Unknown item tag '{}'", normalized(tag)))
        })?;
        return Ok(members.contains(&name));
    }
    Ok(normalized(raw) == name)
}

/// `if items|slots block <pos> <slots> [<item_predicate>]` on a block container: the slots
/// that exist (and hold a matching stack, for `items`); the test passes when there is one.
fn condition_container(
    ctx: &CommandContext<Source>,
    unless: bool,
    with_items: bool,
) -> Result<Source, CommandError> {
    let Some(world) = &ctx.source().world else {
        return passed(ctx, unless, false);
    };
    let pos = block_position(ctx, "source")?;
    let name = world
        .block(pos[0], pos[1], pos[2])
        .and_then(|state| u16::try_from(state).ok())
        .and_then(ferrite_data::blocks::state)
        .map(|(name, _)| normalized(name));
    let Some(size) = name
        .as_deref()
        .and_then(super::block_container::inventory_slots)
    else {
        return passed(ctx, unless, false);
    };
    let slots = super::slot_ops::block_slots(ctx.string("slots").unwrap_or_default(), size)
        .map_err(|bad| CommandError::failed(format!("Unknown slot '{bad}'")))?;
    let contents = super::block_container::load_slots(world, pos, size);
    let mut count = 0;
    for slot in slots {
        let stack = &contents[slot];
        let counts = !with_items
            || (!stack.is_empty()
                && item_matches(ctx.string("item_predicate").unwrap_or("*"), stack.item)?);
        if counts {
            count += 1;
        }
    }
    passed(ctx, unless, count > 0)
}

/// Whether a requested slot name covers a published slot (`armor.*` includes `armor.body`).
fn slot_name_covers(requested: &str, slot: &str) -> bool {
    requested == slot || (requested == "armor.*" && slot.starts_with("armor."))
}

/// `if items|slots entity <targets> <slots> [<item_predicate>]` for entities that are not
/// players. The owning regions publish the item slots they model into the entity snapshot
/// (`EntityIndex::slots_of`: horse armour in `armor.body`); the test reads only that.
///
/// * `items` passes when a selected entity holds a matching stack in a requested slot; a slot
///   the snapshot does not list is empty, which is what the server holds for it.
/// * `slots` passes when a requested slot is published for a selected entity. An entity whose
///   slot set is not modelled cannot be answered, so that case is an error, not a quiet `false`.
/// * Player inventories live in the player's region and are not published: a selection that
///   contains a player and matches nothing fails with a message instead of answering `false`.
fn condition_entity_inventory(
    ctx: &CommandContext<Source>,
    unless: bool,
    with_items: bool,
) -> Result<Source, CommandError> {
    let entities = resolve_in_optional(ctx, "source")?;
    let requested: Vec<&str> = ctx
        .string("slots")
        .unwrap_or_default()
        .split(',')
        .filter(|name| !name.is_empty())
        .collect();
    let predicate = ctx.string("item_predicate").unwrap_or("*");
    let mut matched = false;
    let mut players = false;
    let mut unmodelled = None;
    {
        let source = ctx.source();
        let state = source.state.lock().unwrap();
        let index = state.entities.lock().unwrap();
        for entity in &entities {
            if entity.player {
                players = true;
                continue;
            }
            let published = index.slots_of(entity.uuid);
            let mut any = false;
            for (slot, item) in published {
                if !requested.iter().any(|name| slot_name_covers(name, slot)) {
                    continue;
                }
                any = true;
                if !with_items || item_matches(predicate, *item)? {
                    matched = true;
                }
            }
            if !any && !with_items {
                unmodelled = Some(entity.name.clone());
            }
        }
    }
    if !matched {
        if players {
            return Err(CommandError::failed(
                "execute if items|slots entity cannot read a player's inventory: it is owned by the player's region and not published to commands",
            ));
        }
        if let Some(name) = unmodelled {
            return Err(CommandError::failed(format!(
                "execute if slots entity: the slots of {name} are not modelled by this server"
            )));
        }
    }
    passed(ctx, unless, matched)
}

// ---------------------------------------------------------------------------------------------
// Modifiers.
// ---------------------------------------------------------------------------------------------

fn heightmap(name: &str) -> Option<HeightmapKind> {
    Some(match name {
        "world_surface" => HeightmapKind::WorldSurface,
        "ocean_floor" => HeightmapKind::OceanFloor,
        "motion_blocking" => HeightmapKind::MotionBlocking,
        "motion_blocking_no_leaves" => HeightmapKind::MotionBlockingNoLeaves,
        _ => return None,
    })
}

/// `positioned over <heightmap>`: the y of the first free block above the highest block the
/// heightmap counts at the source's x and z.
pub(super) fn positioned_over(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("over").then(
        argument::<Source>("heightmap", ArgumentType::opaque("minecraft:heightmap")).redirect_with(
            execute,
            |ctx| {
                let raw = ctx.string("heightmap").unwrap_or_default();
                let kind = heightmap(raw)
                    .ok_or_else(|| CommandError::failed(format!("Unknown heightmap '{raw}'")))?;
                let mut source = ctx.source().clone();
                let world = source
                    .world
                    .as_ref()
                    .ok_or_else(|| CommandError::failed("That position is not loaded"))?;
                let x = source.origin[0].floor() as i32;
                let z = source.origin[2].floor() as i32;
                let top = world
                    .heightmap_y(x, z, kind)
                    .ok_or_else(|| CommandError::failed("That position is not loaded"))?;
                source.origin[1] = f64::from(top + 1);
                Ok(source)
            },
        ),
    )
}

/// `on <relation>`: the entity (or entities) related to the executing entity. `vehicle`,
/// `passengers` and `controller` come from the server's ride table; `attacker`, `owner`,
/// `leasher` and `target` come from the region-published entity snapshot. `origin` (the entity
/// an entity was spawned from) is not in that snapshot and fails with a clear message.
pub(super) fn on_branch(execute: NodeId) -> NodeBuilder<Source> {
    let mut on = literal::<Source>("on");
    for relation in ["controller", "passengers", "vehicle"] {
        on = on.then(
            literal::<Source>(relation).fork(execute, move |ctx| relation_sources(ctx, relation)),
        );
    }
    for relation in ["attacker", "leasher", "owner", "target"] {
        on = on.then(
            literal::<Source>(relation).fork(execute, move |ctx| relation_sources(ctx, relation)),
        );
    }
    on = on.then(literal::<Source>("origin").fork(execute, |ctx| relation_sources(ctx, "origin")));
    on
}

fn relation_sources(
    ctx: &CommandContext<Source>,
    relation: &str,
) -> Result<Vec<Source>, CommandError> {
    let source = ctx.source();
    let Some(me) = source.uuid else {
        return Ok(Vec::new());
    };
    let state = source.state.lock().unwrap();
    let related = match relation {
        "vehicle" => super::ride::vehicle_of(me).into_iter().collect::<Vec<_>>(),
        "passengers" => super::ride::passengers_of(me),
        "controller" => super::ride::passengers_of(me).into_iter().take(1).collect(),
        "attacker" => related_uuid(&state, me, |entity| entity.attacker),
        "owner" => related_uuid(&state, me, |entity| entity.owner),
        "leasher" => related_uuid(&state, me, |entity| entity.leasher),
        "target" => related_uuid(&state, me, |entity| entity.target),
        "origin" => state
            .entities
            .lock()
            .unwrap()
            .origin_of(me)
            .into_iter()
            .collect(),
        other => {
            return Err(CommandError::failed(format!(
                "execute on {other} is not supported: the entity snapshot has no {other} data"
            )));
        }
    };
    let mut out = Vec::new();
    for uuid in related {
        let found = super::commands_exec::resolve_optional(
            &state,
            &EntityTarget::Uuid(uuid.to_string()),
            source.uuid,
            source.origin,
        )?;
        out.extend(found.iter().map(|entity| {
            let mut next = source.clone();
            next.uuid = Some(entity.uuid);
            next.name = Some(entity.name.clone());
            next
        }));
    }
    Ok(out)
}

/// One relation UUID from the region-published snapshot, if this entity has one.
fn related_uuid(
    state: &super::commands::GameState,
    uuid: uuid::Uuid,
    field: impl FnOnce(&crate::game::entity_index::CommandEntity) -> Option<uuid::Uuid>,
) -> Vec<uuid::Uuid> {
    state
        .entities
        .lock()
        .unwrap()
        .get(uuid)
        .and_then(field)
        .into_iter()
        .collect()
}

/// `summon <entity>`: a two-phase hand-off, because the region that owns the chunk creates the
/// entity asynchronously.
///
/// Phase one runs here, on the command thread: the entity gets its uuid now, a provisional entry
/// goes into the world-visible snapshot (so `@s`, `as`, `if entity` and the other relations
/// resolve it for the rest of the chain), and `Action::SummonChain` is queued. Phase two runs on
/// the game loop: it schedules the spawn on the owning region with that uuid, and withdraws the
/// provisional entry when the chunk is not loaded. The region keeps the only live state; later
/// actions of the chain reach it through the same ordered queue, after the spawn. The region's
/// first snapshot sync replaces the provisional entry with the real one.
pub(super) fn summon_branch(execute: NodeId) -> NodeBuilder<Source> {
    literal::<Source>("summon").then(
        argument::<Source>("entity", ArgumentType::ResourceLocation).redirect_with(
            execute,
            |ctx| {
                let name = ctx
                    .string("entity")
                    .ok_or_else(|| CommandError::failed("missing entity argument 'entity'"))?;
                let kind = ferrite_entity::mob::MobKind::from_name(name).ok_or_else(|| {
                    if ferrite_data::entity_props::by_name(name).is_some() {
                        CommandError::failed(format!("Unable to summon entity {name}"))
                    } else {
                        CommandError::failed(format!("Invalid entity {name}"))
                    }
                })?;
                let source = ctx.source();
                let uuid = ferrite_entity::random_uuid();
                {
                    let state = source.state.lock().unwrap();
                    state
                        .entities
                        .lock()
                        .unwrap()
                        .insert(super::entity_index::CommandEntity {
                            uuid,
                            network_id: 0,
                            kind: kind.entity_type(),
                            pos: source.origin,
                            yaw: 0.0,
                            pitch: 0.0,
                            dimension: source.dimension,
                            tags: Default::default(),
                            custom_name: None,
                            health: None,
                            attacker: None,
                            owner: None,
                            leasher: None,
                            target: None,
                        });
                }
                if send(
                    source,
                    super::commands::Action::SummonChain {
                        kind,
                        pos: source.origin,
                        uuid,
                        dimension: source.dimension,
                    },
                )
                .is_err()
                {
                    let state = source.state.lock().unwrap();
                    state.entities.lock().unwrap().forget(uuid);
                    return Err(CommandError::failed("command target unavailable"));
                }
                let mut next = source.clone();
                next.uuid = Some(uuid);
                next.name = Some(kind.display_name().to_owned());
                Ok(next)
            },
        ),
    )
}

// ---------------------------------------------------------------------------------------------
// store ... bossbar
// ---------------------------------------------------------------------------------------------

/// `store result|success bossbar <id> value|max`.
pub(super) fn store_bossbar(execute: NodeId, success: bool) -> NodeBuilder<Source> {
    let leaf = move |name: &'static str, max: bool| {
        literal::<Source>(name)
            .redirect_with(execute, |ctx| Ok(ctx.source().clone()))
            .stores(move |ctx, result| {
                let value = if success {
                    i32::from(result != 0)
                } else {
                    result
                };
                write_bossbar(ctx, max, value)
            })
    };
    literal::<Source>("bossbar").then(
        argument::<Source>("id", ArgumentType::ResourceLocation)
            .then(leaf("max", true))
            .then(leaf("value", false)),
    )
}

fn write_bossbar(ctx: &CommandContext<Source>, max: bool, value: i32) -> Result<(), CommandError> {
    use super::commands_extras::PlayerPacket;
    use ferrite_protocol::packets::play::{BossBar, BossBarAction};
    let id = normalized(ctx.string("id").unwrap_or_default());
    let (uuid, progress) = {
        let mut state = ctx.source().state.lock().unwrap();
        let bar = state
            .bossbars
            .get_mut(&id)
            .ok_or_else(|| CommandError::failed(format!("No bossbar exists with the ID '{id}'")))?;
        if max {
            bar.max = value.max(1);
        } else {
            bar.value = value;
        }
        bar.progress = (bar.value as f32 / bar.max as f32).clamp(0.0, 1.0);
        (bar.uuid, bar.progress)
    };
    send(
        ctx.source(),
        super::commands::Action::Packets {
            targets: Vec::new(),
            packet: PlayerPacket::BossBar(BossBar {
                uuid,
                action: BossBarAction::UpdateProgress(progress),
            }),
        },
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::commands::GameState;
    use super::*;
    use crate::game::entity_index::CommandEntity;
    use ferrite_command::EntityAnchor;
    use ferrite_nbt::snbt;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    fn source() -> Source {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::mem::forget(rx);
        let state = Arc::new(Mutex::new(GameState::new(0, 0, 20)));
        state
            .lock()
            .unwrap()
            .join(Uuid::nil(), "Alice".into(), 0, 20);
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
            admin: None,
            tree: None,
            function_depth: 0,
            state,
            actions: tx,
        }
    }

    fn storage(source: &Source, key: &str) -> String {
        snbt::format(source.state.lock().unwrap().storage.get(key).unwrap())
    }

    #[test]
    fn float_ranges_parse_like_vanilla_ranges() {
        assert_eq!(float_range("1.5"), Some((1.5, 1.5)));
        assert_eq!(float_range("1..2"), Some((1.0, 2.0)));
        assert_eq!(float_range("..2"), Some((f64::NEG_INFINITY, 2.0)));
        assert_eq!(float_range("3.."), Some((3.0, f64::INFINITY)));
        assert_eq!(float_range("3..1"), None);
        assert_eq!(float_range("x"), None);
    }

    #[test]
    fn stopwatch_condition_uses_elapsed_seconds() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        let missing = tree
            .execute("execute if stopwatch sw 0..", source.clone())
            .unwrap_err();
        assert!(missing.to_string().contains("does not exist"), "{missing}");
        {
            let mut state = source.state.lock().unwrap();
            state.game_time = 100;
            state.more.stopwatches.insert("minecraft:sw".into(), 20);
        }
        // 80 ticks = 4 seconds.
        tree.execute(
            "execute if stopwatch sw 3..5 run data merge storage t {in:1}",
            source.clone(),
        )
        .unwrap();
        assert_eq!(
            tree.execute(
                "execute if stopwatch sw 5.. run data merge storage t {bad:1}",
                source.clone()
            )
            .unwrap(),
            0
        );
        tree.execute(
            "execute unless stopwatch sw 5.. run data merge storage t {un:1}",
            source.clone(),
        )
        .unwrap();
        assert_eq!(storage(&source, "minecraft:t"), "{in:1,un:1}");
    }

    #[test]
    fn loaded_biome_and_container_conditions_need_a_world() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        for command in [
            "execute if loaded 0 64 0 run say x",
            "execute if biome 0 64 0 minecraft:plains run say x",
            "execute if items block 0 64 0 container.* * run say x",
            "execute if slots block 0 64 0 container.* run say x",
        ] {
            assert_eq!(
                tree.execute(command, source.clone()).unwrap(),
                0,
                "{command}"
            );
        }
        for command in [
            "execute unless loaded 0 64 0 run data merge storage t {a:1}",
            "execute unless biome 0 64 0 #minecraft:is_forest run data merge storage t {b:1}",
        ] {
            tree.execute(command, source.clone()).unwrap();
        }
        assert_eq!(storage(&source, "minecraft:t"), "{a:1,b:1}");
    }

    #[test]
    fn entity_inventory_conditions_read_the_published_slots() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        let iron = ferrite_item::item_id("minecraft:iron_horse_armor").expect("item");
        let horse = Uuid::from_u128(0x41);
        let pig = Uuid::from_u128(0x42);
        {
            let state = source.state.lock().unwrap();
            let mut index = state.entities.lock().unwrap();
            for (uuid, tag) in [(horse, "horse"), (pig, "pig")] {
                index.insert(CommandEntity {
                    uuid,
                    network_id: 1,
                    kind: 0,
                    pos: [1.0, 64.0, 1.0],
                    yaw: 0.0,
                    pitch: 0.0,
                    dimension: crate::game::Dimension::Overworld,
                    tags: [tag.to_owned()].into(),
                    custom_name: None,
                    health: None,
                    attacker: None,
                    owner: None,
                    leasher: None,
                    target: None,
                });
            }
            index.set_slot(horse, "armor.body", iron);
        }
        // `data merge` reports 0 when nothing changes, so every command writes its own key.
        let run = |command: &str, key: usize| {
            tree.execute(
                &command.replace("{a:1}", &format!("{{k{key}:1}}")),
                source.clone(),
            )
        };
        for passing in [
            "execute if items entity @e[tag=horse] armor.body minecraft:iron_horse_armor run data merge storage t {a:1}",
            "execute if items entity @e[tag=horse] armor.* * run data merge storage t {a:1}",
            "execute if slots entity @e[tag=horse] armor.body run data merge storage t {a:1}",
            "execute unless items entity @e[tag=horse] armor.body minecraft:diamond run data merge storage t {a:1}",
            "execute unless items entity @e[tag=pig] armor.body * run data merge storage t {a:1}",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(run(passing.1, passing.0).unwrap(), 1, "{}", passing.1);
        }
        for failing in [
            "execute if items entity @e[tag=horse] armor.body minecraft:diamond run data merge storage t {a:1}",
            "execute if items entity @e[tag=pig] armor.body * run data merge storage t {a:1}",
            "execute if items entity @e[tag=horse] weapon.mainhand * run data merge storage t {a:1}",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(run(failing.1, 10 + failing.0).unwrap(), 0, "{}", failing.1);
        }
        // An entity whose slot set is not modelled is an error for `slots`, not a quiet false.
        let error = run("execute if slots entity @e[tag=pig] armor.body", 20).unwrap_err();
        assert!(error.to_string().contains("not modelled"), "{error}");
        // A player's inventory is not published.
        let error = run("execute if items entity @s weapon.mainhand *", 21).unwrap_err();
        assert!(error.to_string().contains("player's inventory"), "{error}");
    }

    #[test]
    fn function_condition_passes_on_a_nonzero_result() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        // An unknown function never passes.
        assert_eq!(
            tree.execute(
                "execute if function pack:nope run data merge storage t {bad:1}",
                source.clone()
            )
            .unwrap(),
            0
        );
        tree.execute(
            "execute unless function pack:nope run data merge storage t {ok:1}",
            source.clone(),
        )
        .unwrap();
        assert_eq!(storage(&source, "minecraft:t"), "{ok:1}");
    }

    #[test]
    fn on_relations_follow_rides_and_refuse_the_unknown() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        // Alice rides nothing and carries nobody: the branch forks to no sources.
        for relation in ["vehicle", "passengers", "controller"] {
            assert_eq!(
                tree.execute(
                    &format!("execute on {relation} run data merge storage t {{bad:1}}"),
                    source.clone()
                )
                .unwrap(),
                0,
                "{relation}"
            );
        }
        // A player has no snapshot entry, so a relation the snapshot would answer is empty.
        for relation in ["attacker", "leasher", "owner", "target"] {
            assert_eq!(
                tree.execute(
                    &format!("execute on {relation} run data merge storage t {{bad:1}}"),
                    source.clone()
                )
                .unwrap(),
                0,
                "{relation}"
            );
        }
        // `origin` forks like the others: nothing published means no sources, not an error.
        assert_eq!(
            tree.execute(
                "execute on origin run data merge storage t {bad:1}",
                source.clone()
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn on_relations_use_the_published_snapshot() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        let mob = Uuid::from_u128(0x31);
        let related = Uuid::from_u128(0x32);
        let entry = |uuid, tags: &[&str]| CommandEntity {
            uuid,
            network_id: uuid.as_u128() as i32,
            kind: 0,
            pos: [1.0, 64.0, 1.0],
            yaw: 0.0,
            pitch: 0.0,
            dimension: crate::game::Dimension::Overworld,
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            custom_name: None,
            health: None,
            attacker: Some(related),
            owner: Some(related),
            leasher: Some(related),
            target: Some(related),
        };
        {
            let state = source.state.lock().unwrap();
            let mut index = state.entities.lock().unwrap();
            index.insert(entry(mob, &["mob"]));
            index.insert(entry(related, &["related"]));
        }
        source
            .state
            .lock()
            .unwrap()
            .entities
            .lock()
            .unwrap()
            .set_origin(mob, related);
        for relation in ["attacker", "leasher", "owner", "target", "origin"] {
            tree.execute(
                &format!(
                    "execute as @e[tag=mob] on {relation} run data merge storage t {{{relation}:1}}"
                ),
                source.clone(),
            )
            .unwrap();
        }
        assert_eq!(
            storage(&source, "minecraft:t"),
            "{attacker:1,leasher:1,owner:1,target:1,origin:1}"
        );
    }

    #[test]
    fn summon_branch_makes_the_new_entity_the_source_of_the_chain() {
        let tree = super::super::commands::dispatcher();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut source = source();
        source.actions = tx;
        let ran = tree
            .execute(
                "execute summon minecraft:pig if entity @s[type=minecraft:pig] run data merge storage t {pig:1}",
                source.clone(),
            )
            .unwrap();
        assert_eq!(ran, 1, "@s is the provisional pig");
        assert_eq!(storage(&source, "minecraft:t"), "{pig:1}");
        let Ok(super::super::commands::Action::SummonChain {
            kind,
            pos,
            uuid,
            dimension,
        }) = rx.try_recv()
        else {
            panic!("the summon was queued for the owning region");
        };
        assert_eq!(kind, ferrite_entity::mob::MobKind::Pig);
        assert_eq!(pos, [10.0, 64.0, 10.0]);
        assert_eq!(dimension, crate::game::Dimension::Overworld);
        let state = source.state.lock().unwrap();
        let index = state.entities.lock().unwrap();
        assert_eq!(index.get(uuid).map(|entry| entry.pos), Some(pos));
        drop(index);
        drop(state);
        while rx.try_recv().is_ok() {}
        let error = tree
            .execute("execute summon minecraft:nope run say x", source.clone())
            .unwrap_err();
        assert!(error.to_string().contains("Invalid entity"), "{error}");
        assert!(rx.try_recv().is_err(), "an unknown entity queues nothing");
    }

    #[test]
    fn positioned_over_needs_a_loaded_world_and_a_known_heightmap() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        let error = tree
            .execute(
                "execute positioned over world_surface run say x",
                source.clone(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("not loaded"), "{error}");
        assert!(heightmap("motion_blocking_no_leaves").is_some());
        assert!(heightmap("nope").is_none());
    }

    #[test]
    fn store_into_a_bossbar_updates_value_and_max() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        source.state.lock().unwrap().bossbars.insert(
            "minecraft:bar".into(),
            super::super::commands::BossBarState {
                uuid: Uuid::from_u128(7),
                name: "bar".into(),
                progress: 0.0,
                value: 0,
                max: 100,
                color: 0,
                overlay: 0,
                visible: true,
                players: Default::default(),
            },
        );
        tree.execute(
            "execute store result bossbar minecraft:bar max run data merge storage t {a:1}",
            source.clone(),
        )
        .unwrap();
        {
            let state = source.state.lock().unwrap();
            let bar = &state.bossbars["minecraft:bar"];
            assert_eq!(bar.max, 1);
        }
        let error = tree
            .execute(
                "execute store result bossbar minecraft:gone value run data merge storage t {b:1}",
                source.clone(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("No bossbar"), "{error}");
    }

    #[test]
    fn selectors_in_execute_see_non_player_entities() {
        let tree = super::super::commands::dispatcher();
        let source = source();
        source
            .state
            .lock()
            .unwrap()
            .entities
            .lock()
            .unwrap()
            .insert(CommandEntity {
                uuid: Uuid::from_u128(9),
                network_id: 9,
                kind: 0,
                pos: [1.0, 2.0, 3.0],
                yaw: 0.0,
                pitch: 0.0,
                dimension: crate::game::Dimension::Overworld,
                tags: ["boss".to_owned()].into_iter().collect(),
                custom_name: None,
                health: None,
                attacker: None,
                owner: None,
                leasher: None,
                target: None,
            });
        tree.execute(
            "execute as @e[tag=boss] at @s run data merge storage t {seen:1}",
            source.clone(),
        )
        .unwrap();
        assert_eq!(storage(&source, "minecraft:t"), "{seen:1}");
        assert_eq!(
            tree.execute(
                "execute as @e[tag=nobody] run data merge storage t {bad:1}",
                source.clone()
            )
            .unwrap(),
            0
        );
    }

    async fn world_source() -> Source {
        let mut source = source();
        let world = crate::game::anvil::AnvilChunkSource::new_memory();
        while world.get(0, 0, 0).is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let state = |name: &str| u32::from(ferrite_data::blocks::default_state(name).unwrap());
        for x in 0..3 {
            world
                .set_block(x, 200, 0, state("minecraft:stone"))
                .unwrap();
        }
        world
            .set_block(5, 200, 0, state("minecraft:oak_stairs"))
            .unwrap();
        source.world = Some(world);
        source
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_conditions_use_wildcards_tags_and_the_source_for_masked() {
        let tree = super::super::commands::dispatcher();
        let source = world_source().await;
        let hit = |command: &str| tree.execute(command, source.clone()).unwrap();
        assert_eq!(hit("execute if block 0 200 0 minecraft:stone run say x"), 1);
        assert_eq!(
            hit("execute if block 5 200 0 minecraft:oak_stairs run say x"),
            1
        );
        // Unlisted properties are wildcards, so a bare stairs id matches any facing; a listed
        // property must agree.
        assert_eq!(
            hit("execute if block 5 200 0 minecraft:oak_stairs[half=top] run say x"),
            0
        );
        assert_eq!(
            hit("execute if block 5 200 0 #minecraft:stairs run say x"),
            1
        );
        assert_eq!(
            hit("execute if block 0 200 0 #minecraft:stairs run say x"),
            0
        );
        assert_eq!(
            hit("execute unless block 0 200 0 minecraft:dirt run say x"),
            1
        );
        // masked: the source (0..2) is stone, compared with the same stone at 1..3 would fail
        // at 3 (air), but air in the SOURCE is skipped.
        assert_eq!(
            hit("execute if blocks 0 200 0 2 200 0 0 200 0 masked run say x"),
            1
        );
        // Source air (x=3,4) is skipped even though the destination holds stone.
        assert_eq!(
            hit("execute if blocks 3 200 0 4 200 0 0 200 0 masked run say x"),
            1
        );
        assert_eq!(
            hit("execute if blocks 3 200 0 4 200 0 0 200 0 all run say x"),
            0
        );
        assert_eq!(hit("execute if loaded 0 200 0 run say x"), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn positioned_over_uses_the_heightmap() {
        let tree = super::super::commands::dispatcher();
        let mut source = world_source().await;
        source.origin = [1.5, 250.0, 0.5];
        // The stone at y=64 is the highest block, so the position is the block above it: one
        // below that is the stone.
        assert_eq!(
            tree.execute(
                "execute positioned over world_surface if block ~ ~-1 ~ minecraft:stone run say x",
                source.clone()
            )
            .unwrap(),
            1
        );
        assert_eq!(
            tree.execute(
                "execute positioned over world_surface if block ~ ~ ~ minecraft:stone run say x",
                source
            )
            .unwrap(),
            0
        );
    }
}
