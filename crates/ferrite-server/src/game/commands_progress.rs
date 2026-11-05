//! `/advancement` and `/recipe`: per-player progress kept in [`super::commands_more::MoreState`].
//!
//! Ids and the advancement tree (parents, criteria) come from the locally extracted vanilla
//! data. The state is in memory only, and no `update_advancements` / `recipe_book_*` packet is
//! sent yet, so the client shows no toast or recipe-book entry.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument, literal,
};

use super::commands::{Action, Feedback, Source, resolved_targets, send};

fn failure(key: &'static str, args: Vec<String>) -> CommandError {
    CommandError::failed(Feedback::new(key, args).text)
}

fn data_dir() -> Option<PathBuf> {
    let relative = Path::new("vanilla/26.3/generated/data/minecraft");
    [
        relative.to_path_buf(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative),
    ]
    .into_iter()
    .find(|dir| dir.join("advancement").is_dir())
}

fn json_ids(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            json_ids(&path, &format!("{prefix}{name}/"), out);
        } else if let Some(stem) = name.strip_suffix(".json") {
            out.push((format!("minecraft:{prefix}{stem}"), path));
        }
    }
}

/// One advancement: its parent and criterion names.
#[derive(Debug, Clone, Default)]
pub struct Advancement {
    pub parent: Option<String>,
    pub criteria: Vec<String>,
}

fn advancements() -> &'static BTreeMap<String, Advancement> {
    static ALL: OnceLock<BTreeMap<String, Advancement>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut files = Vec::new();
        if let Some(dir) = data_dir() {
            json_ids(&dir.join("advancement"), "", &mut files);
        }
        files
            .into_iter()
            .filter_map(|(id, path)| {
                let json: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
                let criteria = json["criteria"]
                    .as_object()
                    .map(|c| c.keys().cloned().collect())
                    .unwrap_or_default();
                let parent = json["parent"].as_str().map(str::to_owned);
                Some((id, Advancement { parent, criteria }))
            })
            .collect()
    })
}

/// Whether the extracted vanilla data is on this machine.
#[cfg(test)]
pub(super) fn has_data() -> bool {
    !advancements().is_empty()
}

fn recipes() -> &'static BTreeSet<String> {
    static ALL: OnceLock<BTreeSet<String>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut files = Vec::new();
        if let Some(dir) = data_dir() {
            json_ids(&dir.join("recipe"), "", &mut files);
        }
        files.into_iter().map(|(id, _)| id).collect()
    })
}

fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn id_argument(ctx: &CommandContext<Source>, name: &str) -> Result<String, CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::ResourceLocation(id)) => Ok(namespaced(id)),
        _ => Err(CommandError::failed(format!("missing {name}"))),
    }
}

fn player_names(ctx: &CommandContext<Source>, uuids: &[uuid::Uuid]) -> Vec<String> {
    let game = ctx.source().state.lock().unwrap();
    uuids
        .iter()
        .map(|uuid| {
            game.players
                .iter()
                .find(|p| p.uuid == *uuid)
                .map_or_else(|| uuid.to_string(), |p| p.name.clone())
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// /advancement
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Span {
    Only,
    From,
    Until,
    Through,
    Everything,
}

fn descendants(root: &str, out: &mut BTreeSet<String>) {
    out.insert(root.to_owned());
    for (id, adv) in advancements() {
        if adv.parent.as_deref() == Some(root) && !out.contains(id) {
            descendants(id, out);
        }
    }
}

fn ancestors(id: &str, out: &mut BTreeSet<String>) {
    let mut current = Some(id.to_owned());
    while let Some(id) = current {
        if !out.insert(id.clone()) {
            break;
        }
        current = advancements().get(&id).and_then(|a| a.parent.clone());
    }
}

fn selected(span: Span, id: Option<&str>) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    match (span, id) {
        (Span::Everything, _) => set.extend(advancements().keys().cloned()),
        (Span::Only, Some(id)) => {
            set.insert(id.to_owned());
        }
        (Span::From, Some(id)) => descendants(id, &mut set),
        (Span::Until, Some(id)) => ancestors(id, &mut set),
        (Span::Through, Some(id)) => {
            descendants(id, &mut set);
            ancestors(id, &mut set);
        }
        _ => {}
    }
    set
}

fn advancement_run(
    ctx: &CommandContext<Source>,
    grant: bool,
    span: Span,
    criterion: bool,
) -> Result<i32, CommandError> {
    let id = if span == Span::Everything {
        None
    } else {
        let id = id_argument(ctx, "advancement")?;
        if !advancements().contains_key(&id) {
            return Err(failure("advancement.advancementNotFound", vec![id]));
        }
        Some(id)
    };
    let criterion = if criterion {
        let Some(ArgumentValue::String(name)) = ctx.argument("criterion") else {
            return Err(CommandError::failed("missing criterion"));
        };
        let id = id.clone().unwrap_or_default();
        if !advancements()
            .get(&id)
            .is_some_and(|a| a.criteria.contains(name))
        {
            return Err(failure(
                "commands.advancement.criterionNotFound",
                vec![id, name.clone()],
            ));
        }
        Some(name.clone())
    } else {
        None
    };
    let set = selected(span, id.as_deref());
    let targets = resolved_targets(ctx, "targets")?;
    let verb = if grant { "grant" } else { "revoke" };
    let names = player_names(ctx, &targets);
    let mut changed_players = 0;
    let mut changed_advancements = BTreeSet::new();
    {
        let mut game = ctx.source().state.lock().unwrap();
        for uuid in &targets {
            let owned = game.more.advancements.entry(*uuid).or_default();
            let mut any = false;
            for adv in &set {
                let criteria: Vec<String> = match &criterion {
                    Some(name) => vec![name.clone()],
                    None => advancements()
                        .get(adv)
                        .map(|a| a.criteria.clone())
                        .unwrap_or_default(),
                };
                for name in criteria {
                    let key = format!("{adv}#{name}");
                    let did = if grant {
                        owned.insert(key)
                    } else {
                        owned.remove(&key)
                    };
                    if did {
                        any = true;
                        changed_advancements.insert(adv.clone());
                    }
                }
            }
            if any {
                changed_players += 1;
            }
        }
    }
    let many_targets = targets.len() != 1;
    let target_text = if many_targets {
        targets.len().to_string()
    } else {
        names.first().cloned().unwrap_or_default()
    };
    let to = if many_targets { "to.many" } else { "to.one" };
    if changed_players == 0 {
        let key: &'static str = match (criterion.is_some(), set.len() == 1, many_targets, grant) {
            (true, _, false, true) => "commands.advancement.grant.criterion.to.one.failure",
            (true, _, true, true) => "commands.advancement.grant.criterion.to.many.failure",
            (true, _, false, false) => "commands.advancement.revoke.criterion.to.one.failure",
            (true, _, true, false) => "commands.advancement.revoke.criterion.to.many.failure",
            (false, true, false, true) => "commands.advancement.grant.one.to.one.failure",
            (false, true, true, true) => "commands.advancement.grant.one.to.many.failure",
            (false, false, false, true) => "commands.advancement.grant.many.to.one.failure",
            (false, false, true, true) => "commands.advancement.grant.many.to.many.failure",
            (false, true, false, false) => "commands.advancement.revoke.one.to.one.failure",
            (false, true, true, false) => "commands.advancement.revoke.one.to.many.failure",
            (false, false, false, false) => "commands.advancement.revoke.many.to.one.failure",
            (false, false, true, false) => "commands.advancement.revoke.many.to.many.failure",
        };
        let args = match (&criterion, set.len() == 1) {
            (Some(name), _) => vec![name.clone(), id.clone().unwrap_or_default(), target_text],
            (None, true) => vec![set.iter().next().cloned().unwrap_or_default(), target_text],
            (None, false) => vec![set.len().to_string(), target_text],
        };
        return Err(failure(key, args));
    }
    let (key, args): (String, Vec<String>) = match (&criterion, set.len() == 1) {
        (Some(name), _) => (
            format!("commands.advancement.{verb}.criterion.{to}"),
            vec![name.clone(), id.clone().unwrap_or_default(), target_text],
        ),
        (None, true) => (
            format!("commands.advancement.{verb}.one.{to}"),
            vec![set.iter().next().cloned().unwrap_or_default(), target_text],
        ),
        (None, false) => (
            format!("commands.advancement.{verb}.many.{to}"),
            vec![changed_advancements.len().to_string(), target_text],
        ),
    };
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(Box::leak(key.into_boxed_str()), args)),
    )?;
    Ok(changed_players)
}

fn advancement_tree() -> NodeBuilder<Source> {
    fn span(
        name: &'static str,
        grant: bool,
        which: Span,
        with_criterion: bool,
    ) -> NodeBuilder<Source> {
        let adv = argument::<Source>("advancement", ArgumentType::ResourceLocation)
            .executes(move |ctx| advancement_run(ctx, grant, which, false));
        let adv = if with_criterion {
            adv.then(
                argument::<Source>("criterion", ArgumentType::word())
                    .executes(move |ctx| advancement_run(ctx, grant, which, true)),
            )
        } else {
            adv
        };
        literal::<Source>(name).then(adv)
    }
    fn side(verb: &'static str, grant: bool) -> NodeBuilder<Source> {
        literal::<Source>(verb).then(
            argument::<Source>("targets", ArgumentType::players())
                .then(span("only", grant, Span::Only, true))
                .then(span("from", grant, Span::From, false))
                .then(span("until", grant, Span::Until, false))
                .then(span("through", grant, Span::Through, false))
                .then(
                    literal::<Source>("everything")
                        .executes(move |ctx| advancement_run(ctx, grant, Span::Everything, false)),
                ),
        )
    }
    literal::<Source>("advancement")
        .requires(|s| s.permission >= 2)
        .then(side("grant", true))
        .then(side("revoke", false))
}

// ---------------------------------------------------------------------------------------------
// /recipe
// ---------------------------------------------------------------------------------------------

fn recipe_run(ctx: &CommandContext<Source>, give: bool, all: bool) -> Result<i32, CommandError> {
    let wanted: Vec<String> = if all {
        recipes().iter().cloned().collect()
    } else {
        let id = id_argument(ctx, "recipe")?;
        if !recipes().contains(&id) {
            return Err(failure("recipe.notFound", vec![id]));
        }
        vec![id]
    };
    let targets = resolved_targets(ctx, "targets")?;
    let names = player_names(ctx, &targets);
    let mut changed = 0usize;
    {
        let mut game = ctx.source().state.lock().unwrap();
        for uuid in &targets {
            let known = game.more.recipes.entry(*uuid).or_default();
            for id in &wanted {
                let did = if give {
                    known.insert(id.clone())
                } else {
                    known.remove(id)
                };
                if did {
                    changed += 1;
                }
            }
        }
    }
    let many = targets.len() != 1;
    let target_text = if many {
        targets.len().to_string()
    } else {
        names.first().cloned().unwrap_or_default()
    };
    if changed == 0 {
        return Err(failure(
            if give {
                "commands.recipe.give.failed"
            } else {
                "commands.recipe.take.failed"
            },
            vec![],
        ));
    }
    let key = match (give, many) {
        (true, false) => "commands.recipe.give.success.single",
        (true, true) => "commands.recipe.give.success.multiple",
        (false, false) => "commands.recipe.take.success.single",
        (false, true) => "commands.recipe.take.success.multiple",
    };
    send(
        ctx.source(),
        Action::Feedback(Feedback::new(key, vec![changed.to_string(), target_text])),
    )?;
    Ok(i32::try_from(changed).unwrap_or(i32::MAX))
}

fn recipe_tree() -> NodeBuilder<Source> {
    fn side(verb: &'static str, give: bool) -> NodeBuilder<Source> {
        literal::<Source>(verb).then(
            argument::<Source>("targets", ArgumentType::players())
                .then(literal::<Source>("*").executes(move |ctx| recipe_run(ctx, give, true)))
                .then(
                    argument::<Source>("recipe", ArgumentType::ResourceLocation)
                        .executes(move |ctx| recipe_run(ctx, give, false)),
                ),
        )
    }
    literal::<Source>("recipe")
        .requires(|s| s.permission >= 2)
        .then(side("give", true))
        .then(side("take", false))
}

pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(advancement_tree());
    tree.register(recipe_tree());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancement_spans_follow_the_parent_chain() {
        let all = advancements();
        if all.is_empty() {
            return; // no extracted vanilla data on this machine
        }
        let from = selected(Span::From, Some("minecraft:story/root"));
        assert!(from.contains("minecraft:story/mine_stone"));
        let until = selected(Span::Until, Some("minecraft:story/mine_stone"));
        assert_eq!(
            until,
            BTreeSet::from([
                "minecraft:story/mine_stone".to_owned(),
                "minecraft:story/root".to_owned()
            ])
        );
        assert!(selected(Span::Everything, None).len() > 100);
    }
}
