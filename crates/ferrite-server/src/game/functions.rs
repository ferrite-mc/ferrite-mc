//! Datapack functions: `.mcfunction` loading, tags, macros and the `/function` command.
//!
//! The loader reads `<world>/datapacks/*/data/<namespace>/function/**/*.mcfunction` and the
//! matching `tags/function` JSON files, so a world can ship a datapack without a jar. A
//! function line runs through the server command tree on the calling thread and its actions
//! join the caller's action channel, exactly like a command typed by that source.

use super::commands::Source;
use ferrite_command::{
    ArgumentType, CommandContext, CommandDispatcher, CommandError, NodeBuilder, argument, literal,
};
use ferrite_nbt::{Compound, Tag, snbt};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The vanilla tag run once after startup.
pub const LOAD_TAG: &str = "minecraft:load";
/// The vanilla tag run at the start of every tick.
pub const TICK_TAG: &str = "minecraft:tick";
/// Vanilla's guard against a function that calls itself forever.
const MAX_DEPTH: u32 = 64;

/// One parsed `.mcfunction` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Function {
    /// Command lines, with comments, blank lines and continuation backslashes removed.
    pub lines: Vec<String>,
    /// Whether at least one line starts with `$` and takes macro arguments.
    pub macros: bool,
}

impl Function {
    /// Parses a function file. A line ending in `\` continues on the next line; lines whose
    /// first non-space character is `#` are comments.
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut macros = false;
        let mut pending: Option<String> = None;
        for raw in text.lines() {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            if pending.is_none() {
                let trimmed = raw.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
            }
            let (piece, continued) = match raw.trim_end().strip_suffix('\\') {
                Some(piece) => (piece, true),
                None => (raw, false),
            };
            let joined = match pending.take() {
                Some(mut line) => {
                    line.push_str(piece);
                    line
                }
                None => piece.to_owned(),
            };
            if continued {
                pending = Some(joined);
            } else {
                push_line(&mut lines, &joined, &mut macros);
            }
        }
        if let Some(line) = pending {
            push_line(&mut lines, &line, &mut macros);
        }
        Self { lines, macros }
    }

    /// The command lines to run, with macro arguments substituted.
    fn expand(&self, arguments: Option<&Compound>) -> Result<Vec<String>, CommandError> {
        if !self.macros {
            return Ok(self.lines.clone());
        }
        let Some(arguments) = arguments else {
            return Err(CommandError::failed(
                "A macro function needs a compound argument",
            ));
        };
        self.lines
            .iter()
            .map(|line| {
                if line.starts_with('$') {
                    substitute(line, arguments).map_err(CommandError::failed)
                } else {
                    Ok(line.clone())
                }
            })
            .collect()
    }
}

fn push_line(lines: &mut Vec<String>, line: &str, macros: &mut bool) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }
    if line.starts_with('$') {
        *macros = true;
    }
    lines.push(line.to_owned());
}

/// Replaces every `$(name)` in a macro line with the matching compound value. The leading
/// `$` itself is removed. Strings are inserted verbatim; numbers and other tags use their
/// SNBT form.
pub fn substitute(line: &str, arguments: &Compound) -> Result<String, String> {
    let text = line.strip_prefix('$').unwrap_or(line);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("$(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            return Err(format!("Unclosed macro in '{line}'"));
        };
        let name = &after[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("Invalid macro name '$({name})'"));
        }
        let Some(value) = arguments.get(name) else {
            return Err(format!("No argument named '{name}' for macro '$({name})'"));
        };
        out.push_str(&argument_text(value));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn argument_text(tag: &Tag) -> String {
    match tag {
        Tag::String(text) => text.clone(),
        Tag::Byte(value) => value.to_string(),
        Tag::Short(value) => value.to_string(),
        Tag::Int(value) => value.to_string(),
        Tag::Long(value) => value.to_string(),
        Tag::Float(value) => value.to_string(),
        Tag::Double(value) => value.to_string(),
        other => snbt::format(other),
    }
}

/// Every function and function tag loaded from one world's `datapacks` folder.
#[derive(Debug, Clone, Default)]
pub struct FunctionStore {
    functions: BTreeMap<String, Function>,
    tags: BTreeMap<String, Vec<String>>,
}

impl FunctionStore {
    /// Loads `<world>/datapacks/*`, later packs overriding earlier ones. A world without a
    /// datapacks folder (or a memory-only world) gives an empty store.
    pub fn load(world: Option<&Path>) -> Self {
        Self::load_with_disabled(world, &BTreeSet::new())
    }

    /// Loads every pack except the disabled ones. `/datapack disable` persists its choice in
    /// `<world>/datapacks/.ferrite-disabled` and reloads through this function.
    pub fn load_with_disabled(world: Option<&Path>, disabled: &BTreeSet<String>) -> Self {
        let mut store = Self::default();
        let Some(packs) = world.map(|world| world.join("datapacks")) else {
            return store;
        };
        let Ok(entries) = std::fs::read_dir(&packs) else {
            return store;
        };
        let mut packs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !disabled.contains(name))
            })
            .collect();
        packs.sort();
        for pack in packs {
            store.load_pack(&pack);
        }
        store
    }

    fn load_pack(&mut self, pack: &Path) {
        let data = pack.join("data");
        let Ok(namespaces) = std::fs::read_dir(&data) else {
            return;
        };
        let mut namespaces: Vec<PathBuf> = namespaces
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        namespaces.sort();
        for namespace in namespaces {
            let Some(name) = namespace
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            for (relative, path) in files_in(&namespace.join("function"), "mcfunction") {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        self.functions
                            .insert(format!("{name}:{relative}"), Function::parse(&text));
                    }
                    Err(error) => {
                        tracing::warn!(%error, path = %path.display(), "reading function failed");
                    }
                }
            }
            for (relative, path) in files_in(&namespace.join("tags/function"), "json") {
                self.load_tag(&format!("{name}:{relative}"), &path);
            }
        }
    }

    fn load_tag(&mut self, id: &str, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            tracing::warn!(path = %path.display(), "invalid function tag");
            return;
        };
        let Some(values) = value.get("values").and_then(serde_json::Value::as_array) else {
            return;
        };
        let replace = value
            .get("replace")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let tag = self.tags.entry(id.to_owned()).or_default();
        if replace {
            tag.clear();
        }
        for value in values {
            let id = match value {
                serde_json::Value::String(id) => Some(id.as_str()),
                serde_json::Value::Object(map) => map.get("id").and_then(serde_json::Value::as_str),
                _ => None,
            };
            // Nested `#tag` entries are not expanded yet.
            if let Some(id) = id.filter(|id| !id.starts_with('#')) {
                tag.push(namespaced(id));
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    pub fn tag(&self, name: &str) -> Option<&[String]> {
        self.tags.get(name).map(Vec::as_slice)
    }

    pub fn function_names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }

    pub fn tag_names(&self) -> impl Iterator<Item = &str> {
        self.tags.keys().map(String::as_str)
    }
}

/// Folder names of every data pack in `<world>/datapacks`.
pub fn datapack_names(world: Option<&Path>) -> Vec<String> {
    let Some(world) = world else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(world.join("datapacks")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        // `.ferrite-disabled` is this server's own marker file, not a pack.
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn disabled_path(world: &Path) -> PathBuf {
    world.join("datapacks/.ferrite-disabled")
}

/// Pack folder names disabled by `/datapack disable`.
pub fn read_disabled(world: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(disabled_path(world))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Persists disabled pack names. Ferrite has no `level.dat` data-pack settings yet, so this
/// small marker file is the host-side equivalent.
pub fn write_disabled(world: &Path, disabled: &BTreeSet<String>) -> std::io::Result<()> {
    let dir = world.join("datapacks");
    std::fs::create_dir_all(&dir)?;
    let text = disabled.iter().cloned().collect::<Vec<_>>().join("\n");
    std::fs::write(disabled_path(world), text)
}

/// Reloads the function store from disk, honouring the persisted disabled pack list.
pub fn reload(
    world: Option<&Path>,
    state: &Arc<std::sync::Mutex<super::commands::GameState>>,
) -> Result<(), String> {
    let disabled = world.map(read_disabled).unwrap_or_default();
    let store = if disabled.is_empty() {
        FunctionStore::load(world)
    } else {
        FunctionStore::load_with_disabled(world, &disabled)
    };
    state.lock().unwrap().functions = Arc::new(store);
    Ok(())
}

fn files_in(dir: &Path, extension: &str) -> Vec<(String, PathBuf)> {
    fn visit(dir: &Path, prefix: &str, extension: &str, out: &mut Vec<(String, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                visit(&path, &format!("{prefix}{name}/"), extension, out);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                out.push((format!("{prefix}{stem}"), path));
            }
        }
    }
    let mut out = Vec::new();
    visit(dir, "", extension, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn namespaced(raw: &str) -> String {
    if raw.contains(':') {
        raw.to_owned()
    } else {
        format!("minecraft:{raw}")
    }
}

fn unknown(name: &str) -> CommandError {
    CommandError::failed(super::feedback::render(
        "commands.function.unknown",
        &[name],
    ))
}

/// Runs a `/function` argument: `ns:name`, `name`, or `#ns:tag`.
pub fn run_command(
    source: &Source,
    raw: &str,
    arguments: Option<&Compound>,
) -> Result<i32, CommandError> {
    let depth = source.function_depth + 1;
    if let Some(tag) = raw.strip_prefix('#') {
        let name = namespaced(tag);
        let functions = {
            let state = source.state.lock().unwrap();
            state.functions.tag(&name).map(<[String]>::to_vec)
        };
        let Some(functions) = functions else {
            return Err(unknown(&format!("#{name}")));
        };
        let mut result = 0;
        for function in functions {
            result = run(source, &function, arguments, depth)?;
        }
        return Ok(result);
    }
    run(source, &namespaced(raw), arguments, depth)
}

/// Runs one function by id. `return` stops the function and becomes its result.
pub fn run(
    source: &Source,
    name: &str,
    arguments: Option<&Compound>,
    depth: u32,
) -> Result<i32, CommandError> {
    if depth > MAX_DEPTH {
        return Err(CommandError::failed(format!(
            "Function call depth exceeded {MAX_DEPTH}"
        )));
    }
    let function = {
        let state = source.state.lock().unwrap();
        state.functions.get(name).cloned()
    };
    let Some(function) = function else {
        return Err(unknown(name));
    };
    let lines = function.expand(arguments)?;
    let tree = source
        .tree
        .as_ref()
        .ok_or_else(|| CommandError::failed("functions need a running command dispatcher"))?;
    let mut nested = source.clone();
    nested.function_depth = depth;
    let mut result = 0;
    for line in lines {
        let line = line.trim_start();
        if line == "return" {
            return Err(CommandError::failed("return needs a value"));
        }
        if let Some(value) = line.strip_prefix("return ") {
            return evaluate_return(tree, &nested, value.trim());
        }
        result = tree.execute(line, nested.clone())?;
    }
    Ok(result)
}

fn evaluate_return(
    tree: &CommandDispatcher<Source>,
    source: &Source,
    rest: &str,
) -> Result<i32, CommandError> {
    if let Some(command) = rest.strip_prefix("run ") {
        return tree.execute(command, source.clone());
    }
    rest.parse::<i32>()
        .map_err(|_| CommandError::failed(format!("'{rest}' is not a valid integer")))
}

/// Adds `/function` and `/return` to the server tree.
pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(function_builder());
    tree.register(return_builder());
}

fn function_builder() -> NodeBuilder<Source> {
    literal::<Source>("function")
        .requires(|source| source.permission >= 2)
        .restricted(true)
        .then(
            argument::<Source>("name", ArgumentType::opaque("minecraft:function"))
                .suggests(|ctx, builder| {
                    let state = ctx.source().state.lock().unwrap();
                    for name in state.functions.function_names() {
                        builder.suggest(name.to_owned());
                    }
                    for name in state.functions.tag_names() {
                        builder.suggest(format!("#{name}"));
                    }
                })
                .executes(execute)
                .then(
                    argument::<Source>(
                        "arguments",
                        ArgumentType::opaque("minecraft:nbt_compound_tag"),
                    )
                    .executes(execute_with_arguments),
                ),
        )
}

fn name_arg(ctx: &CommandContext<Source>) -> Result<&str, CommandError> {
    ctx.string("name")
        .ok_or_else(|| CommandError::failed("Missing function name"))
}

fn execute(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    run_command(ctx.source(), name_arg(ctx)?, None)
}

fn execute_with_arguments(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let arguments = match snbt::parse(ctx.string("arguments").unwrap_or_default()) {
        Ok(Tag::Compound(compound)) => compound,
        Ok(_) => {
            return Err(CommandError::failed(
                "Function arguments must be a compound tag",
            ));
        }
        Err(error) => return Err(CommandError::failed(error.to_string())),
    };
    run_command(ctx.source(), name_arg(ctx)?, Some(&arguments))
}

fn return_builder() -> NodeBuilder<Source> {
    literal::<Source>("return")
        .requires(|source| source.permission >= 2)
        .restricted(true)
        .then(argument::<Source>("value", ArgumentType::integer()).executes(not_in_function))
        .then(literal("run").then(
            argument::<Source>("command", ArgumentType::greedy_string()).executes(not_in_function),
        ))
}

fn not_in_function(_: &CommandContext<Source>) -> Result<i32, CommandError> {
    Err(CommandError::failed(super::feedback::render(
        "commands.function.return.not_in_function",
        &[],
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::commands::{Action, GameState, Source};
    use std::sync::{Arc, Mutex};

    fn source(
        tree: &Arc<CommandDispatcher<Source>>,
        functions: FunctionStore,
    ) -> (Source, tokio::sync::mpsc::UnboundedReceiver<Action>) {
        let (actions, received) = tokio::sync::mpsc::unbounded_channel();
        let mut state = GameState::new(1, 0, 20);
        state.functions = Arc::new(functions);
        (
            Source {
                permission: 4,
                origin: [0.0, 64.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: None,
                name: None,
                anchor: ferrite_command::EntityAnchor::Feet,
                dimension: crate::game::Dimension::Overworld,
                world: None,
                state: Arc::new(Mutex::new(state)),
                actions,
                tree: Some(Arc::clone(tree)),
                function_depth: 0,
            },
            received,
        )
    }

    fn store(functions: &[(&str, &str)]) -> FunctionStore {
        let mut store = FunctionStore::default();
        for (name, text) in functions {
            store
                .functions
                .insert((*name).to_owned(), Function::parse(text));
        }
        store
    }

    fn compound(text: &str) -> Compound {
        match snbt::parse(text).unwrap() {
            Tag::Compound(compound) => compound,
            other => panic!("expected a compound, got {other:?}"),
        }
    }

    #[test]
    fn parser_skips_comments_and_blank_lines_and_joins_continuations() {
        let function =
            Function::parse("# a comment\n\n  # indented comment\nsay one \\\n two\nsay three\n");
        assert_eq!(
            function.lines,
            vec!["say one  two".to_owned(), "say three".to_owned()]
        );
        assert!(!function.macros);
    }

    #[test]
    fn loader_reads_datapack_functions_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("datapacks/pack/data");
        std::fs::create_dir_all(data.join("demo/function/nested")).unwrap();
        std::fs::write(
            data.join("demo/function/nested/greet.mcfunction"),
            "# greeting\n$say hi $(name)\n",
        )
        .unwrap();
        std::fs::create_dir_all(data.join("minecraft/tags/function")).unwrap();
        std::fs::write(
            data.join("minecraft/tags/function/tick.json"),
            r#"{"values":["demo:nested/greet",{"id":"demo:other","required":false}]}"#,
        )
        .unwrap();
        let store = FunctionStore::load(Some(dir.path()));
        let greet = store.get("demo:nested/greet").expect("loaded function");
        assert_eq!(greet.lines, vec!["$say hi $(name)".to_owned()]);
        assert!(greet.macros);
        assert_eq!(
            store.tag("minecraft:tick").unwrap().to_vec(),
            vec!["demo:nested/greet".to_owned(), "demo:other".to_owned()]
        );
    }

    #[test]
    fn macros_substitute_compound_arguments() {
        let function = Function::parse("$say $(name) has $(count)\n");
        let arguments = compound("{name:\"Steve\",count:3}");
        assert_eq!(
            function.expand(Some(&arguments)).unwrap(),
            vec!["say Steve has 3".to_owned()]
        );
        assert!(function.expand(None).is_err());
        assert!(
            function
                .expand(Some(&compound("{name:\"Steve\"}")))
                .is_err()
        );
    }

    #[test]
    fn function_command_unknown_function_errors() {
        let tree = Arc::new(crate::game::commands::dispatcher());
        let (source, _received) = source(&tree, FunctionStore::default());
        let error = tree
            .execute("function test:missing", source)
            .expect_err("unknown function must fail");
        assert!(error.to_string().contains("Unknown function"), "{error}");
    }

    #[test]
    fn function_command_substitutes_macros_and_return_stops_the_function() {
        let functions = store(&[
            ("test:greet", "$say hello $(who)"),
            ("test:stop", "say before\nreturn 7\nsay after"),
        ]);
        let tree = Arc::new(crate::game::commands::dispatcher());
        let (source, mut received) = source(&tree, functions);
        tree.execute("function test:greet {who:\"Alex\"}", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(
            matches!(
                actions.as_slice(),
                [Action::Broadcast(feedback)] if feedback.text.contains("hello Alex")
            ),
            "{actions:?}"
        );
        let result = tree.execute("function test:stop", source).unwrap();
        assert_eq!(result, 7);
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert!(
            matches!(&actions[0], Action::Broadcast(feedback) if feedback.text.contains("before"))
        );
    }
}
