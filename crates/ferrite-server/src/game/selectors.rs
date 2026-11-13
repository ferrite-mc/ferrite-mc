//! Entity selector evaluation against the command-visible player and entity state.
//!
//! The parser in `ferrite-command` validates selector shape; this module resolves the selected
//! entities and implements the vanilla options that need world state. Players come from
//! [`GameState::players`], every other entity from the region-published [`EntityIndex`].
//! Folia note: the index is a snapshot. Resolution picks UUIDs; the mutation itself is
//! scheduled on the region that owns the entity (see `game::mod`).

use super::commands::GameState;
use ferrite_command::arguments::{EntityTarget, GameMode, SelectorKind};
use ferrite_command::{CommandError, CommandSyntaxError};
use ferrite_nbt::{Compound, Tag, snbt};
use std::collections::BTreeSet;
use uuid::Uuid;

/// One resolved entity. `player` distinguishes the mailbox route from the region route.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedEntity {
    pub uuid: Uuid,
    pub name: String,
    pub player: bool,
    /// Entity type protocol id; `None` for players.
    pub kind: Option<u16>,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// A player or region entity plus the state selector options filter on.
struct Candidate {
    selected: SelectedEntity,
    /// `None` for players.
    kind: Option<u16>,
    gamemode: Option<i32>,
    tags: BTreeSet<String>,
    team: Option<String>,
    level: i32,
}

impl Candidate {
    fn type_name(&self) -> &'static str {
        self.kind.map_or("minecraft:player", |kind| {
            ferrite_data::entity_props::get(kind).key
        })
    }

    fn distance_squared(&self, origin: [f64; 3]) -> f64 {
        self.selected
            .pos
            .iter()
            .zip(origin)
            .map(|(a, b)| (a - b) * (a - b))
            .sum()
    }

    /// The subset of entity NBT selectors can match, in vanilla's field names.
    fn nbt(&self) -> Option<Tag> {
        let mut text = format!(
            "{{id:\"{}\",Pos:[{},{},{}],Rotation:[{},{}]",
            self.type_name(),
            self.selected.pos[0],
            self.selected.pos[1],
            self.selected.pos[2],
            self.selected.yaw,
            self.selected.pitch
        );
        if !self.tags.is_empty() {
            text.push_str(",Tags:[");
            for (index, tag) in self.tags.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                text.push('"');
                text.push_str(&tag.replace('\\', "\\\\").replace('"', "\\\""));
                text.push('"');
            }
            text.push(']');
        }
        if self.selected.player {
            text.push_str(&format!(
                ",playerGameType:{},XpLevel:{}",
                self.gamemode.unwrap_or(0),
                self.level
            ));
        }
        text.push('}');
        snbt::parse(&text).ok()
    }
}

fn error(key: &'static str, message: impl Into<String>) -> CommandError {
    CommandError::Syntax(CommandSyntaxError::new(key, Vec::new(), message.into()))
}

fn split_negated(raw: &str) -> (bool, &str) {
    match raw.strip_prefix('!') {
        Some(value) => (true, value),
        None => (false, raw),
    }
}

fn normalized_type(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn range_f64(raw: &str) -> Option<(f64, f64)> {
    let raw = raw.trim();
    match raw.split_once("..") {
        Some((min, max)) => {
            let min = if min.is_empty() {
                f64::NEG_INFINITY
            } else {
                min.parse().ok()?
            };
            let max = if max.is_empty() {
                f64::INFINITY
            } else {
                max.parse().ok()?
            };
            Some((min, max))
        }
        None => {
            let value: f64 = raw.parse().ok()?;
            Some((value, value))
        }
    }
}

fn range_i32(raw: &str) -> Option<(i32, i32)> {
    let raw = raw.trim();
    match raw.split_once("..") {
        Some((min, max)) => {
            let min = if min.is_empty() {
                i32::MIN
            } else {
                min.parse().ok()?
            };
            let max = if max.is_empty() {
                i32::MAX
            } else {
                max.parse().ok()?
            };
            Some((min, max))
        }
        None => {
            let value: i32 = raw.parse().ok()?;
            Some((value, value))
        }
    }
}

fn parse_scores(raw: &str) -> Option<Vec<(&str, (i32, i32))>> {
    let inner = raw.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut scores = Vec::new();
    for entry in inner.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (objective, value) = entry.split_once('=')?;
        scores.push((objective.trim(), range_i32(value)?));
    }
    Some(scores)
}

fn number(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Byte(value) => Some(f64::from(*value)),
        Tag::Short(value) => Some(f64::from(*value)),
        Tag::Int(value) => Some(f64::from(*value)),
        Tag::Long(value) => Some(*value as f64),
        Tag::Float(value) => Some(f64::from(*value)),
        Tag::Double(value) => Some(*value),
        _ => None,
    }
}

/// Vanilla `NbtPredicate.matches`: compound keys are a subset, list lengths must agree, and
/// numbers compare within the 1e-5 tolerance vanilla uses for floating point NBT.
fn nbt_matches(expected: &Tag, actual: &Tag) -> bool {
    if let (Some(expected), Some(actual)) = (number(expected), number(actual)) {
        return (expected - actual).abs() < 1e-5;
    }
    match (expected, actual) {
        (Tag::Compound(expected), Tag::Compound(actual)) => expected
            .iter()
            .all(|(key, value)| actual.get(key).is_some_and(|a| nbt_matches(value, a))),
        // A typed list has no generic iterator; vanilla's list predicate is an exact value
        // match for the tags entity selectors can build here (Pos/Rotation/Tags).
        _ => expected == actual,
    }
}

fn candidates(state: &GameState) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut players = BTreeSet::new();
    for player in &state.players {
        players.insert(player.uuid);
        candidates.push(Candidate {
            selected: SelectedEntity {
                uuid: player.uuid,
                name: player.name.clone(),
                player: true,
                kind: None,
                pos: player.pos,
                yaw: player.yaw,
                pitch: player.pitch,
            },
            kind: None,
            gamemode: Some(player.game_mode),
            tags: player.tags.clone(),
            team: player.team.clone(),
            level: player.xp_level,
        });
    }
    // Regions publish players too; the player list above is the authoritative command view.
    for entity in state.entities.lock().unwrap().entries() {
        if players.contains(&entity.uuid) {
            continue;
        }
        candidates.push(Candidate {
            selected: SelectedEntity {
                uuid: entity.uuid,
                name: entity.display_name(),
                player: false,
                kind: Some(entity.kind),
                pos: entity.pos,
                yaw: entity.yaw,
                pitch: entity.pitch,
            },
            kind: Some(entity.kind),
            gamemode: None,
            tags: entity.tags.clone(),
            team: None,
            level: 0,
        });
    }
    candidates
}

fn not_found(target: &EntityTarget) -> CommandError {
    match target {
        EntityTarget::Name(_) => error("argument.entity.notfound.player", "No player was found"),
        EntityTarget::Uuid(_) => error("argument.entity.notfound.entity", "No entity was found"),
        EntityTarget::Selector { kind, .. } => match kind {
            SelectorKind::Myself | SelectorKind::AllEntities | SelectorKind::NearestEntity => {
                error("argument.entity.notfound.entity", "No entity was found")
            }
            _ => error("argument.entity.notfound.player", "No player was found"),
        },
    }
}

fn option_error(key: &'static str, option: &str) -> CommandError {
    error(key, format!("Invalid value for selector option '{option}'"))
}

/// Resolves an entity argument to the entities it selects.
///
/// `source` is the executing player (for `@s`), `origin` the source position for relative
/// selector options such as `distance` and `dx`/`dy`/`dz`.
pub fn resolve_selection(
    state: &GameState,
    target: &EntityTarget,
    source: Option<Uuid>,
    origin: [f64; 3],
) -> Result<Vec<SelectedEntity>, CommandError> {
    let all = candidates(state);
    let chosen: Vec<Candidate> = match target {
        EntityTarget::Name(name) => all
            .into_iter()
            .filter(|candidate| candidate.selected.name.eq_ignore_ascii_case(name))
            .collect(),
        EntityTarget::Uuid(raw) => {
            let Ok(uuid) = Uuid::parse_str(raw) else {
                return Err(not_found(target));
            };
            all.into_iter()
                .filter(|candidate| candidate.selected.uuid == uuid)
                .collect()
        }
        EntityTarget::Selector { kind, options } => {
            let mut chosen: Vec<Candidate> = match kind {
                SelectorKind::Myself => all
                    .into_iter()
                    .filter(|candidate| Some(candidate.selected.uuid) == source)
                    .collect(),
                SelectorKind::NearestPlayer
                | SelectorKind::AllPlayers
                | SelectorKind::RandomPlayer => all
                    .into_iter()
                    .filter(|candidate| candidate.selected.player)
                    .collect(),
                SelectorKind::AllEntities | SelectorKind::NearestEntity => all,
            };
            apply_options(&mut chosen, options, state, origin)?;

            // `@p`, `@r` and `@n` are shorthand for a sorted, limited `@e`; explicit options
            // still override the default sort and limit.
            let explicit_sort = options.iter().any(|option| option.key == "sort");
            let explicit_limit = options.iter().any(|option| option.key == "limit");
            if !explicit_sort {
                match kind {
                    SelectorKind::NearestPlayer
                    | SelectorKind::NearestEntity
                    | SelectorKind::RandomPlayer => {
                        if *kind != SelectorKind::RandomPlayer {
                            chosen.sort_by(|a, b| {
                                a.distance_squared(origin)
                                    .total_cmp(&b.distance_squared(origin))
                            });
                        } else {
                            choose_one_random(&mut chosen);
                        }
                    }
                    _ => {}
                }
            }
            if !explicit_limit
                && *kind != SelectorKind::AllPlayers
                && *kind != SelectorKind::AllEntities
            {
                chosen.truncate(1);
            }
            chosen
        }
    };

    if chosen.is_empty() {
        return Err(not_found(target));
    }
    Ok(chosen
        .into_iter()
        .map(|candidate| candidate.selected)
        .collect())
}

fn choose_one_random(chosen: &mut [Candidate]) {
    if chosen.len() > 1 {
        let index = (Uuid::new_v4().as_u128() % chosen.len() as u128) as usize;
        chosen.swap(0, index);
    }
}

fn apply_options(
    chosen: &mut Vec<Candidate>,
    options: &[ferrite_command::arguments::SelectorOption],
    state: &GameState,
    origin: [f64; 3],
) -> Result<(), CommandError> {
    let mut sort = "arbitrary".to_owned();
    let mut limit: Option<usize> = None;
    let mut volume = false;

    for option in options {
        let raw = option.value.trim();
        match option.key.as_str() {
            "name" => {
                let (negated, name) = split_negated(raw);
                let name = name.trim_matches('"');
                chosen.retain(|candidate| {
                    candidate.selected.name.eq_ignore_ascii_case(name) != negated
                });
            }
            "distance" => {
                let Some((min, max)) = range_f64(raw) else {
                    return Err(option_error(
                        "argument.entity.options.distance.invalid",
                        raw,
                    ));
                };
                chosen.retain(|candidate| {
                    let distance = candidate.distance_squared(origin).sqrt();
                    distance >= min && distance <= max
                });
            }
            "level" => {
                let Some((min, max)) = range_i32(raw) else {
                    return Err(option_error("argument.entity.options.level.invalid", raw));
                };
                chosen.retain(|candidate| {
                    candidate.selected.player && (min..=max).contains(&candidate.level)
                });
            }
            "x" | "y" | "z" | "dx" | "dy" | "dz" => volume = true,
            "x_rotation" => {
                let Some((min, max)) = range_f64(raw) else {
                    return Err(option_error(
                        "argument.entity.options.x_rotation.invalid",
                        raw,
                    ));
                };
                chosen.retain(|candidate| {
                    let pitch = f64::from(candidate.selected.pitch);
                    pitch >= min && pitch <= max
                });
            }
            "y_rotation" => {
                let Some((min, max)) = range_f64(raw) else {
                    return Err(option_error(
                        "argument.entity.options.y_rotation.invalid",
                        raw,
                    ));
                };
                chosen.retain(|candidate| {
                    let yaw = f64::from(candidate.selected.yaw);
                    yaw >= min && yaw <= max
                });
            }
            "gamemode" => {
                let Some(mode) = GameMode::from_name(raw) else {
                    return Err(option_error(
                        "argument.entity.options.gamemode.invalid",
                        raw,
                    ));
                };
                let id = match mode {
                    GameMode::Survival => 0,
                    GameMode::Creative => 1,
                    GameMode::Adventure => 2,
                    GameMode::Spectator => 3,
                };
                chosen.retain(|candidate| candidate.gamemode == Some(id));
            }
            "team" => {
                let (negated, team) = split_negated(raw);
                chosen.retain(|candidate| {
                    candidate.team.as_deref().is_some_and(|value| value == team) != negated
                });
            }
            "type" => {
                let (negated, kind) = split_negated(raw);
                if let Some(tag_name) = kind.strip_prefix('#') {
                    // Vanilla resolves `type=#tag` through the built-in entity_type tags, e.g.
                    // `@e[type=#minecraft:zombies]` and its negated form.
                    let Some(tag) = ferrite_data::tags::get("entity_type", tag_name) else {
                        return Err(option_error("argument.entity.options.type.invalid", raw));
                    };
                    chosen.retain(|candidate| tag.contains(candidate.type_name()) != negated);
                } else {
                    let kind = normalized_type(kind);
                    if ferrite_data::entity_props::by_name(&kind).is_none() {
                        return Err(option_error("argument.entity.options.type.invalid", raw));
                    }
                    chosen.retain(|candidate| (candidate.type_name() == kind) != negated);
                }
            }
            "tag" => {
                let (negated, tag) = split_negated(raw);
                chosen.retain(|candidate| candidate.tags.contains(tag) != negated);
            }
            "scores" => {
                let Some(scores) = parse_scores(raw) else {
                    return Err(option_error("argument.entity.options.scores.invalid", raw));
                };
                for (objective, (min, max)) in scores {
                    chosen.retain(|candidate| {
                        state
                            .scores
                            .get(&(candidate.selected.name.clone(), objective.to_owned()))
                            .is_some_and(|value| (min..=max).contains(value))
                    });
                }
            }
            "nbt" => {
                let Ok(expected) = snbt::parse(raw) else {
                    return Err(option_error("argument.entity.options.nbt.invalid", raw));
                };
                chosen.retain(|candidate| {
                    candidate
                        .nbt()
                        .is_some_and(|actual| nbt_matches(&expected, &actual))
                });
            }
            "sort" => match raw {
                "nearest" | "furthest" | "random" | "arbitrary" => sort = raw.to_owned(),
                _ => return Err(option_error("argument.entity.options.sort.invalid", raw)),
            },
            "limit" => {
                let Ok(parsed) = raw.parse::<usize>() else {
                    return Err(option_error("argument.entity.options.limit.invalid", raw));
                };
                limit = Some(parsed.max(1));
            }
            "advancements" | "predicate" => {
                return Err(error(
                    "argument.entity.options.unsupported",
                    format!(
                        "The '{}' selector option needs the advancements/predicate subsystem, \
                         which Ferrite does not have yet",
                        option.key
                    ),
                ));
            }
            _ => {}
        }
    }

    if volume {
        let coordinate = |key: &str| {
            options
                .iter()
                .find(|option| option.key == key)
                .and_then(|option| option.value.trim().parse::<f64>().ok())
        };
        let min = [
            coordinate("x").unwrap_or(origin[0]),
            coordinate("y").unwrap_or(origin[1]),
            coordinate("z").unwrap_or(origin[2]),
        ];
        let max = [
            min[0] + coordinate("dx").unwrap_or(0.0),
            min[1] + coordinate("dy").unwrap_or(0.0),
            min[2] + coordinate("dz").unwrap_or(0.0),
        ];
        chosen.retain(|candidate| {
            let pos = candidate.selected.pos;
            pos[0] >= min[0]
                && pos[0] <= max[0]
                && pos[1] >= min[1]
                && pos[1] <= max[1]
                && pos[2] >= min[2]
                && pos[2] <= max[2]
        });
    }

    match sort.as_str() {
        "nearest" => chosen.sort_by(|a, b| {
            a.distance_squared(origin)
                .total_cmp(&b.distance_squared(origin))
        }),
        "furthest" => chosen.sort_by(|a, b| {
            b.distance_squared(origin)
                .total_cmp(&a.distance_squared(origin))
        }),
        "random" => {
            for index in (1..chosen.len()).rev() {
                let swap = (Uuid::new_v4().as_u128() % (index as u128 + 1)) as usize;
                chosen.swap(index, swap);
            }
        }
        _ => {}
    }
    if let Some(limit) = limit {
        chosen.truncate(limit);
    }
    Ok(())
}

/// A compound with one field, for tests and `/data`-style callers.
#[allow(dead_code)]
pub(crate) fn compound(fields: impl IntoIterator<Item = (String, Tag)>) -> Tag {
    let mut compound = Compound::new();
    for (key, value) in fields {
        compound.insert(key, value);
    }
    Tag::Compound(compound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::commands::GameState;
    use crate::game::entity_index::CommandEntity;
    use ferrite_command::StringReader;
    use ferrite_command::arguments::entity::parse_entity;

    fn target(raw: &str) -> EntityTarget {
        parse_entity(&mut StringReader::new(raw.to_owned()), false, false)
            .unwrap_or_else(|error| panic!("{raw}: {error}"))
    }

    fn state() -> (GameState, Uuid, Uuid) {
        let mut state = GameState::default();
        let alice = Uuid::new_v4();
        state.join(alice, "Alice".into(), 1, 20);
        state.set_position(alice, [0.5, 64.0, 0.5], 0.0, 0.0);

        let zombie = Uuid::new_v4();
        let kind = ferrite_data::entity_props::by_name("zombie").unwrap().id;
        state.entities.lock().unwrap().insert(CommandEntity {
            uuid: zombie,
            network_id: 7,
            kind,
            pos: [3.5, 64.0, 4.5],
            yaw: 0.0,
            pitch: 0.0,
            dimension: crate::game::Dimension::Overworld,
            tags: BTreeSet::from(["boss".to_owned()]),
            custom_name: Some("Boss".into()),
        });
        (state, alice, zombie)
    }

    #[test]
    fn attribute_limits_follow_the_selected_entity_type() {
        use crate::game::attributes::limits_for;
        let (state, alice, zombie) = state();
        let selected = resolve_selection(&state, &target("@e"), None, [0.0; 3]).unwrap();
        let find = |uuid| selected.iter().find(|entity| entity.uuid == uuid).unwrap();
        assert_eq!(
            limits_for(find(alice), "attack_damage").unwrap().default,
            1.0
        );
        assert_eq!(
            limits_for(find(zombie), "attack_damage").unwrap().default,
            3.0
        );
        assert_eq!(limits_for(find(zombie), "minecraft:nope"), None);
    }

    #[test]
    fn all_entities_include_players_and_region_entities() {
        let (state, alice, zombie) = state();
        let selected = resolve_selection(&state, &target("@e"), None, [0.0; 3]).unwrap();
        assert_eq!(selected.len(), 2);
        assert!(
            selected
                .iter()
                .any(|entity| entity.uuid == alice && entity.player)
        );
        assert!(
            selected
                .iter()
                .any(|entity| entity.uuid == zombie && !entity.player)
        );
    }

    #[test]
    fn type_distance_limit_and_sort() {
        let (state, _, zombie) = state();
        let selected = resolve_selection(
            &state,
            &target("@e[type=zombie,distance=..10,limit=1,sort=nearest]"),
            None,
            [0.0, 64.0, 0.0],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].uuid, zombie);

        let nearest = resolve_selection(
            &state,
            &target("@n[type=zombie,sort=furthest]"),
            None,
            [0.0, 64.0, 0.0],
        )
        .unwrap();
        assert_eq!(nearest[0].uuid, zombie);
    }

    #[test]
    fn negation_tags_scores_volume_rotation_and_nbt() {
        let (mut state, alice, _) = state();
        {
            let mut entity = state
                .entities
                .lock()
                .unwrap()
                .entries()
                .next()
                .unwrap()
                .clone();
            entity.custom_name = None;
            entity.tags.insert("arena".into());
            entity.pos = [2.0, 64.0, 2.0];
            state.entities.lock().unwrap().insert(entity);
        }
        // Only the player is not a zombie here.
        let selected =
            resolve_selection(&state, &target("@e[type=!zombie]"), None, [0.0; 3]).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].uuid, alice);

        // Entity tags, volume and NBT.
        let selected = resolve_selection(
            &state,
            &target("@e[tag=arena,dx=4,dy=2,dz=4,nbt={id:\"minecraft:zombie\"}]"),
            None,
            [0.0, 63.0, 0.0],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert!(!selected[0].player);

        // Scores and teams apply to players.
        state.players[0].team = Some("red".into());
        state.scores.insert(("Alice".into(), "points".into()), 3);
        let selected = resolve_selection(
            &state,
            &target("@a[team=red,scores={points=1..5},gamemode=creative]"),
            None,
            [0.0; 3],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        let selected = resolve_selection(&state, &target("@a[team=!red]"), None, [0.0; 3]);
        assert!(selected.is_err());
    }

    #[test]
    fn invalid_options_and_empty_selections_are_errors() {
        let (state, _, _) = state();
        for raw in [
            "@e[type=not_a_real_entity]",
            "@e[distance=far]",
            "@e[sort=sideways]",
            "@e[nbt={oops}]",
            "@e[level=1..2]",
        ] {
            assert!(
                resolve_selection(&state, &target(raw), None, [0.0; 3]).is_err(),
                "{raw} should select nothing"
            );
        }
        // Selector permission is enforced by the dispatcher, not here.
        assert!(resolve_selection(&state, &target("@s"), None, [0.0; 3]).is_err());
    }

    #[test]
    fn entity_type_tags_match_and_negate() {
        let (state, alice, zombie) = state();
        let selected = resolve_selection(
            &state,
            &target("@e[type=#minecraft:zombies]"),
            None,
            [0.0; 3],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].uuid, zombie);

        let selected = resolve_selection(
            &state,
            &target("@e[type=!#minecraft:zombies]"),
            None,
            [0.0; 3],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].uuid, alice);

        // Unknown tags and unknown entity types are argument errors, not empty selections.
        for raw in [
            "@e[type=#minecraft:not_a_tag]",
            "@e[type=not_a_real_entity]",
        ] {
            let error = resolve_selection(&state, &target(raw), None, [0.0; 3]).unwrap_err();
            let CommandError::Syntax(error) = error else {
                panic!("{raw} should be a syntax error");
            };
            assert_eq!(
                error.translation_key(),
                "argument.entity.options.type.invalid"
            );
        }
    }
}
