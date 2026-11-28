//! `CommandDispatcher`: owns the node graph and implements Brigadier's parse, execute,
//! completion, usage and ambiguity algorithms.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::Arc;

use crate::context::{CommandContext, ParsedArgument};
use crate::error::{CommandError, CommandSyntaxError, builtin, minecraft};
use crate::reader::{ARGUMENT_SEPARATOR, StringReader};
use crate::suggestion::{StringRange, Suggestions, SuggestionsBuilder};
use crate::tree::{Command, Node, NodeBuilder, NodeId, NodeKind, Requirement};

/// The outcome of [`CommandDispatcher::parse`].
pub struct ParseResults<S> {
    pub context: CommandContext<S>,
    /// Positioned where parsing stopped; anything left is unparsed input.
    pub reader: StringReader,
    /// Errors of the nodes that were tried at the point parsing stopped.
    pub errors: Vec<(NodeId, CommandSyntaxError)>,
}

impl<S> ParseResults<S> {
    /// Whether the whole input was consumed.
    pub fn is_complete(&self) -> bool {
        !self.reader.can_read()
    }
}

/// Two sibling nodes that both accept `inputs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ambiguity {
    pub parent: NodeId,
    pub child: NodeId,
    pub sibling: NodeId,
    pub inputs: Vec<String>,
}

/// A command graph generic over the command source `S`.
pub struct CommandDispatcher<S> {
    nodes: Vec<Node<S>>,
    selector_requirement: Option<Requirement<S>>,
}

impl<S> Default for CommandDispatcher<S> {
    fn default() -> Self {
        Self::new()
    }
}

const ROOT: NodeId = NodeId(0);

impl<S> CommandDispatcher<S> {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::new(NodeKind::Root)],
            selector_requirement: None,
        }
    }

    pub fn root(&self) -> NodeId {
        ROOT
    }

    pub fn node(&self, id: NodeId) -> &Node<S> {
        &self.nodes[id.index()]
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Node<S> {
        &mut self.nodes[id.index()]
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The root's literal children, in registration order (`/help` lists these).
    pub fn root_literals(&self) -> Vec<(&str, NodeId)> {
        self.node(ROOT)
            .children()
            .iter()
            .filter_map(|id| match self.node(*id).kind() {
                NodeKind::Literal(name) => Some((name.as_str(), *id)),
                _ => None,
            })
            .collect()
    }

    /// The first full usage string below `id`, e.g. `tp <targets> <location>`.
    pub fn first_usage(&self, id: NodeId) -> Option<String> {
        fn walk<S>(
            dispatcher: &CommandDispatcher<S>,
            id: NodeId,
            path: &mut Vec<String>,
            result: &mut Option<String>,
        ) {
            if result.is_some() {
                return;
            }
            let node = dispatcher.node(id);
            if node.is_executable() {
                *result = Some(path.join(" "));
                return;
            }
            for child in node.children() {
                path.push(dispatcher.node(*child).usage_text());
                walk(dispatcher, *child, path, result);
                path.pop();
            }
        }
        let mut result = None;
        walk(self, id, &mut Vec::new(), &mut result);
        result
    }

    /// Adds a top-level command, merging with an existing one of the same name.
    pub fn register(&mut self, builder: NodeBuilder<S>) -> NodeId {
        self.add_child(ROOT, builder)
    }

    /// Adds `builder` under `parent`. Like Brigadier's `addChild`, a child with the same
    /// name is merged: its command is replaced if the new one has one, and the new
    /// node's children are merged recursively.
    pub fn add_child(&mut self, parent: NodeId, builder: NodeBuilder<S>) -> NodeId {
        let name = match &builder.kind {
            NodeKind::Root => panic!("cannot add a root node as a child"),
            NodeKind::Literal(l) => l.clone(),
            NodeKind::Argument { name, .. } => name.clone(),
        };
        let NodeBuilder {
            kind,
            children,
            command,
            requirement,
            restricted,
            redirect,
            modifier,
            forks,
            suggester,
            store,
        } = builder;
        let id = if let Some(existing) = self.node(parent).child(&name) {
            if command.is_some() {
                self.node_mut(existing).command = command;
            }
            existing
        } else {
            let mut node = Node::new(kind);
            node.command = command;
            node.requirement = requirement;
            node.restricted = restricted;
            node.redirect = redirect;
            node.modifier = modifier;
            node.forks = forks;
            node.suggester = suggester;
            node.store = store;
            self.insert_node(parent, node)
        };
        for child in children {
            self.add_child(id, child);
        }
        id
    }

    /// Appends `node` as a new child of `parent` (no merging).
    pub(crate) fn insert_node(&mut self, parent: NodeId, node: Node<S>) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("too many nodes"));
        let name = node.name().to_owned();
        let is_literal = matches!(node.kind, NodeKind::Literal(_));
        self.nodes.push(node);
        let p = self.node_mut(parent);
        p.children.push(id);
        p.by_name.insert(name.clone(), id);
        if is_literal {
            p.literals.insert(name, id);
        } else {
            p.arguments.push(id);
        }
        id
    }

    pub fn set_command(&mut self, node: NodeId, command: Option<Command<S>>) {
        self.node_mut(node).command = command;
    }

    pub fn set_requirement(&mut self, node: NodeId, requirement: Option<Requirement<S>>) {
        self.node_mut(node).requirement = requirement;
    }

    pub fn set_restricted(&mut self, node: NodeId, restricted: bool) {
        self.node_mut(node).restricted = restricted;
    }

    /// Gates entity selectors on `requirement`, like vanilla's `commands/entity_selectors`
    /// permission. When set, any entity, game-profile or score-holder argument that is a
    /// selector (`@a`, `@e[...]`, `@s`, ...) fails to parse for a source that does not pass
    /// `requirement`, for every command in the tree.
    pub fn set_selector_requirement(
        &mut self,
        requirement: impl Fn(&S) -> bool + Send + Sync + 'static,
    ) {
        self.selector_requirement = Some(Arc::new(requirement));
    }

    /// Sets (or clears) a plain redirect. Panics if the node has children.
    pub fn set_redirect(&mut self, node: NodeId, target: Option<NodeId>) {
        let n = self.node_mut(node);
        assert!(
            n.children.is_empty() || target.is_none(),
            "cannot redirect a node with children"
        );
        n.redirect = target;
    }

    /// Follows child names from the root.
    pub fn find_node<P: AsRef<str>>(&self, path: &[P]) -> Option<NodeId> {
        path.iter()
            .try_fold(ROOT, |node, name| self.node(node).child(name.as_ref()))
    }

    /// Names from the root to `target` (breadth-first, like Brigadier's `getPath`).
    pub fn path(&self, target: NodeId) -> Option<Vec<String>> {
        let mut queue = std::collections::VecDeque::from([(ROOT, Vec::<String>::new())]);
        let mut seen = HashSet::from([ROOT]);
        while let Some((node, path)) = queue.pop_front() {
            if node == target {
                return Some(path);
            }
            for &child in &self.node(node).children {
                if seen.insert(child) {
                    let mut p = path.clone();
                    p.push(self.node(child).name().to_owned());
                    queue.push_back((child, p));
                }
            }
        }
        None
    }

    fn relevant_nodes(&self, node: NodeId, reader: &StringReader) -> Vec<NodeId> {
        let n = self.node(node);
        if !n.literals.is_empty() {
            let rest = reader.remaining();
            let word = rest.split(ARGUMENT_SEPARATOR).next().unwrap_or("");
            if let Some(&literal) = n.literals.get(word) {
                return vec![literal];
            }
        }
        n.arguments.clone()
    }

    fn parse_node(
        &self,
        id: NodeId,
        reader: &mut StringReader,
        context: &mut CommandContext<S>,
    ) -> Result<(), CommandSyntaxError> {
        let node = self.node(id);
        let start = reader.cursor();
        match &node.kind {
            NodeKind::Root => {}
            NodeKind::Literal(literal) => {
                let end = start + literal.len();
                let rest = reader.remaining();
                let matches = rest.starts_with(literal.as_str())
                    && rest[literal.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| c == ARGUMENT_SEPARATOR);
                if !matches {
                    return Err(builtin::literal_incorrect(reader, literal));
                }
                reader.set_cursor(end);
            }
            NodeKind::Argument { name, ty } => {
                let value = ty.parse(reader)?;
                if value.has_selector()
                    && self
                        .selector_requirement
                        .as_ref()
                        .is_some_and(|requirement| !requirement(&context.source))
                {
                    let mut at_start = reader.clone();
                    at_start.set_cursor(start);
                    return Err(minecraft::selectors_not_allowed(&at_start));
                }
                let range = StringRange::new(start, reader.cursor());
                context
                    .arguments
                    .insert(name.clone(), ParsedArgument { range, value });
            }
        }
        let range = StringRange::new(start, reader.cursor());
        context.with_node(id, range, node.modifier.clone(), node.forks);
        Ok(())
    }

    /// Whether `source` passes the requirement of `node`.
    fn can_use(&self, node: NodeId, source: &S) -> bool {
        self.node(node).can_use(source)
    }

    /// Whether `input` would be accepted by `node` as a whole argument (for ambiguities).
    fn is_valid_input(&self, node: NodeId, input: &str) -> bool {
        let mut reader = StringReader::new(input.to_owned());
        match &self.node(node).kind {
            NodeKind::Root => false,
            NodeKind::Literal(l) => {
                input.starts_with(l.as_str())
                    && input[l.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| c == ARGUMENT_SEPARATOR)
            }
            NodeKind::Argument { ty, .. } => {
                ty.parse(&mut reader).is_ok()
                    && reader.peek().is_none_or(|c| c == ARGUMENT_SEPARATOR)
            }
        }
    }

    fn examples(&self, node: NodeId) -> Vec<String> {
        match &self.node(node).kind {
            NodeKind::Root => Vec::new(),
            NodeKind::Literal(l) => vec![l.clone()],
            NodeKind::Argument { ty, .. } => ty.examples().iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Sibling pairs where one sibling accepts an example of the other
    /// (Brigadier's `findAmbiguities`).
    pub fn find_ambiguities(&self) -> Vec<Ambiguity> {
        let mut out = Vec::new();
        let mut stack = vec![ROOT];
        let mut seen = HashSet::new();
        while let Some(parent) = stack.pop() {
            if !seen.insert(parent) {
                continue;
            }
            let children = &self.node(parent).children;
            for &child in children {
                for &sibling in children {
                    if child == sibling {
                        continue;
                    }
                    let inputs: Vec<String> = self
                        .examples(child)
                        .into_iter()
                        .filter(|input| self.is_valid_input(sibling, input))
                        .collect();
                    if !inputs.is_empty() {
                        out.push(Ambiguity {
                            parent,
                            child,
                            sibling,
                            inputs,
                        });
                    }
                }
                stack.push(child);
            }
        }
        out
    }

    /// Every full usage string under `node` (Brigadier's `getAllUsage`).
    pub fn all_usage(&self, node: NodeId, source: &S, restricted: bool) -> Vec<String> {
        let mut out = Vec::new();
        self.all_usage_into(node, source, &mut out, String::new(), restricted);
        out
    }

    fn all_usage_into(
        &self,
        id: NodeId,
        source: &S,
        out: &mut Vec<String>,
        prefix: String,
        restricted: bool,
    ) {
        let node = self.node(id);
        if restricted && !node.can_use(source) {
            return;
        }
        if node.command.is_some() {
            out.push(prefix.clone());
        }
        if let Some(target) = node.redirect {
            let redirect = self.redirect_usage(target);
            out.push(if prefix.is_empty() {
                format!("{} {redirect}", node.usage_text())
            } else {
                format!("{prefix} {redirect}")
            });
        } else {
            for &child in &node.children {
                let text = self.node(child).usage_text();
                let next = if prefix.is_empty() {
                    text
                } else {
                    format!("{prefix} {text}")
                };
                self.all_usage_into(child, source, out, next, restricted);
            }
        }
    }

    fn redirect_usage(&self, target: NodeId) -> String {
        if target == ROOT {
            "...".into()
        } else {
            format!("-> {}", self.node(target).usage_text())
        }
    }

    /// Compact usage per usable child of `node` (Brigadier's `getSmartUsage`), e.g.
    /// `gamemode <gamemode> [<target>]`.
    pub fn smart_usage(&self, node: NodeId, source: &S) -> Vec<(NodeId, String)> {
        let optional = self.node(node).command.is_some();
        self.node(node)
            .children
            .iter()
            .filter_map(|&child| {
                self.smart_usage_of(child, source, optional, false)
                    .map(|u| (child, u))
            })
            .collect()
    }

    fn smart_usage_of(&self, id: NodeId, source: &S, optional: bool, deep: bool) -> Option<String> {
        let node = self.node(id);
        if !node.can_use(source) {
            return None;
        }
        let this = if optional {
            format!("[{}]", node.usage_text())
        } else {
            node.usage_text()
        };
        if deep {
            return Some(this);
        }
        let child_optional = node.command.is_some();
        let (open, close) = if child_optional {
            ("[", "]")
        } else {
            ("(", ")")
        };
        if let Some(target) = node.redirect {
            return Some(format!("{this} {}", self.redirect_usage(target)));
        }
        let children: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|&c| self.can_use(c, source))
            .collect();
        match children.len() {
            0 => {}
            1 => {
                if let Some(usage) =
                    self.smart_usage_of(children[0], source, child_optional, child_optional)
                {
                    return Some(format!("{this} {usage}"));
                }
            }
            _ => {
                let mut usages: Vec<String> = Vec::new();
                for &child in &children {
                    if let Some(u) = self.smart_usage_of(child, source, child_optional, true)
                        && !usages.contains(&u)
                    {
                        usages.push(u);
                    }
                }
                if usages.len() == 1 {
                    let usage = &usages[0];
                    return Some(if child_optional {
                        format!("{this} [{usage}]")
                    } else {
                        format!("{this} {usage}")
                    });
                } else if usages.len() > 1 {
                    let alternatives: Vec<String> = children
                        .iter()
                        .map(|&c| self.node(c).usage_text())
                        .collect();
                    return Some(format!("{this} {open}{}{close}", alternatives.join("|")));
                }
            }
        }
        Some(this)
    }
}

/// Signals that a `/return` command inside a function halted the current command execution
/// (including any remaining branches of an `execute` fork).
pub fn halt_command(value: i32) {
    HALTED.with(|slot| *slot.borrow_mut() = Some(value));
}

fn take_halted() -> Option<i32> {
    HALTED.with(|slot| slot.borrow_mut().take())
}

thread_local! {
    static HALTED: std::cell::RefCell<Option<i32>> = const { std::cell::RefCell::new(None) };
}

impl<S: Clone> CommandDispatcher<S> {
    /// Parses `input` for `source` without running anything.
    pub fn parse(&self, input: &str, source: S) -> ParseResults<S> {
        let input: Arc<str> = input.into();
        let reader = StringReader::new(input.clone());
        let context = CommandContext::new(source, input, ROOT, 0);
        self.parse_nodes(ROOT, &reader, context)
    }

    fn parse_nodes(
        &self,
        node: NodeId,
        original: &StringReader,
        context_so_far: CommandContext<S>,
    ) -> ParseResults<S> {
        let mut errors = Vec::new();
        let mut potentials: Vec<ParseResults<S>> = Vec::new();
        for child in self.relevant_nodes(node, original) {
            if !self.can_use(child, &context_so_far.source) {
                continue;
            }
            let mut context = context_so_far.clone();
            let mut reader = original.clone();
            let parsed = self
                .parse_node(child, &mut reader, &mut context)
                .and_then(|()| {
                    if reader.peek().is_some_and(|c| c != ARGUMENT_SEPARATOR) {
                        Err(builtin::expected_argument_separator(&reader))
                    } else {
                        Ok(())
                    }
                });
            if let Err(e) = parsed {
                errors.push((child, e));
                continue;
            }
            let child_node = self.node(child);
            context.command = child_node.command.clone();
            let redirect = child_node.redirect;
            if reader.can_read_chars(if redirect.is_none() { 2 } else { 1 }) {
                reader.skip();
                if let Some(target) = redirect {
                    let child_context = CommandContext::new(
                        context_so_far.source.clone(),
                        context_so_far.input.clone(),
                        target,
                        reader.cursor(),
                    );
                    let parse = self.parse_nodes(target, &reader, child_context);
                    context.child = Some(Box::new(parse.context));
                    return ParseResults {
                        context,
                        reader: parse.reader,
                        errors: parse.errors,
                    };
                }
                potentials.push(self.parse_nodes(child, &reader, context));
            } else {
                potentials.push(ParseResults {
                    context,
                    reader,
                    errors: Vec::new(),
                });
            }
        }
        if potentials.is_empty() {
            return ParseResults {
                context: context_so_far,
                reader: original.clone(),
                errors,
            };
        }
        // Prefer fully consumed input, then error-free parses (stable, like Java's sort).
        potentials.sort_by_key(|p| (p.reader.can_read(), !p.errors.is_empty()));
        potentials.swap_remove(0)
    }

    /// Parses and runs `input`.
    pub fn execute(&self, input: &str, source: S) -> Result<i32, CommandError> {
        let parse = self.parse(input, source);
        self.execute_parsed(&parse)
    }

    /// Runs a parse result (Brigadier's `execute(ParseResults)`), following redirects
    /// and forks. A fork returns the number of successful branches; otherwise the sum of
    /// command results.
    pub fn execute_parsed(&self, parse: &ParseResults<S>) -> Result<i32, CommandError> {
        if parse.reader.can_read() {
            return Err(match parse.errors.as_slice() {
                [(_, e)] => e.clone(),
                _ if parse.context.range.is_empty() => builtin::unknown_command(&parse.reader),
                _ => builtin::unknown_argument(&parse.reader),
            }
            .into());
        }
        let _ = take_halted();
        let _budget = BudgetGuard::enter();
        let (result, found_command) = self.run_context(&parse.context)?;
        let result = take_halted().unwrap_or(result);
        if !found_command {
            return Err(builtin::unknown_command(&parse.reader).into());
        }
        Ok(result)
    }

    /// Runs one parse level and everything redirected from it. A fork skips failing
    /// branches; every other failure ends the command.
    fn run_context(
        &self,
        context: &crate::context::CommandContext<S>,
    ) -> Result<(i32, bool), CommandError> {
        let mut result = 0i32;
        let mut successful = 0i32;
        let mut found_command = false;
        let mut forked = false;
        if let Some(child) = context.child.as_deref() {
            if child.has_nodes() {
                found_command = true;
                forked = context.forks;
                let sources = match &context.modifier {
                    None => vec![context.source.clone()],
                    Some(modifier) => match modifier(context) {
                        Ok(sources) => sources,
                        Err(_) if forked => Vec::new(),
                        Err(e) => return Err(e),
                    },
                };
                if forked {
                    charge_forks(sources.len() as u64)?;
                }
                for source in sources {
                    match self.run_context(&child.copy_for(source)) {
                        Ok((value, _)) => {
                            if let Some(halted) = HALTED.with(|slot| *slot.borrow()) {
                                result = halted;
                                successful += 1;
                                forked = false;
                                break;
                            }
                            result = result.wrapping_add(value);
                            successful += 1;
                        }
                        Err(_) if forked => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        } else if let Some(command) = &context.command {
            found_command = true;
            charge_sequence()?;
            result = command(context)?;
        }
        if let Some(parsed) = context.nodes.last()
            && let Some(store) = &self.node(parsed.node).store
        {
            store(context, result)?;
        }
        Ok((if forked { successful } else { result }, found_command))
    }

    /// Completions for the input of `parse` at byte `cursor`
    /// (Brigadier's `getCompletionSuggestions`). Providers receive the context level the
    /// cursor is in, with its input truncated at the cursor.
    pub fn completion_suggestions(&self, parse: &ParseResults<S>, cursor: usize) -> Suggestions {
        let full = parse.reader.string();
        let cursor = cursor.min(full.len());
        let (parent, start, level) = find_suggestion_context(&parse.context, cursor);
        let start = start.min(cursor);
        let truncated = &full[..cursor];
        let mut context = level.clone();
        context.input = truncated.into();
        let mut all = Vec::new();
        for &child in &self.node(parent).children {
            let node = self.node(child);
            if !node.can_use(&context.source) {
                continue;
            }
            let mut builder = SuggestionsBuilder::new(truncated, start);
            match &node.kind {
                NodeKind::Root => {}
                NodeKind::Literal(literal) => {
                    builder.suggest_matching(literal.clone());
                }
                NodeKind::Argument { ty, .. } => {
                    match node.suggester.as_ref().and_then(|s| s.provider.as_ref()) {
                        Some(provider) => provider(&context, &mut builder),
                        None => ty.list_suggestions(&mut builder),
                    }
                }
            }
            all.push(builder.build());
        }
        Suggestions::merge(truncated, all)
    }

    /// Parses `input` and completes at its end.
    pub fn suggest(&self, input: &str, source: S) -> Suggestions {
        let parse = self.parse(input, source);
        self.completion_suggestions(&parse, input.len())
    }
}

/// The per-top-level-command budget shared by nested function calls and `execute` forks.
#[derive(Clone, Copy, Default)]
struct Budget {
    sequence: u64,
    forks: u64,
}

thread_local! {
    static BUDGET: Cell<Option<Budget>> = const { Cell::new(None) };
}

/// Installs a budget for the outermost `execute`; nested `tree.execute` calls reuse it.
struct BudgetGuard {
    outermost: bool,
}

impl BudgetGuard {
    fn enter() -> Self {
        let outermost = BUDGET.with(|slot| {
            if slot.get().is_none() {
                slot.set(Some(Budget::default()));
                true
            } else {
                false
            }
        });
        Self { outermost }
    }
}

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        if self.outermost {
            BUDGET.with(|slot| slot.set(None));
        }
    }
}

fn charge_sequence() -> Result<(), CommandError> {
    let limit = crate::limits::max_sequence();
    let used = BUDGET.with(|slot| {
        let mut budget = slot.get().unwrap_or_default();
        budget.sequence = budget.sequence.saturating_add(1);
        slot.set(Some(budget));
        budget.sequence
    });
    // `0` disables chained commands but still lets the directly typed command run.
    if limit != u64::MAX && used > limit.max(1) {
        return Err(CommandError::failed(format!(
            "Command sequence length exceeded {limit}"
        )));
    }
    Ok(())
}

fn charge_forks(count: u64) -> Result<(), CommandError> {
    let limit = crate::limits::max_forks();
    let used = BUDGET.with(|slot| {
        let mut budget = slot.get().unwrap_or_default();
        budget.forks = budget.forks.saturating_add(count);
        slot.set(Some(budget));
        budget.forks
    });
    if limit != u64::MAX && used > limit {
        return Err(CommandError::failed(format!(
            "Command fork limit exceeded {limit}"
        )));
    }
    Ok(())
}

fn find_suggestion_context<S>(
    ctx: &CommandContext<S>,
    cursor: usize,
) -> (NodeId, usize, &CommandContext<S>) {
    if ctx.range.start <= cursor {
        if ctx.range.end < cursor {
            if let Some(child) = ctx.child.as_deref() {
                return find_suggestion_context(child, cursor);
            }
            if let Some(last) = ctx.nodes.last() {
                return (last.node, last.range.end + 1, ctx);
            }
            return (ctx.root, ctx.range.start, ctx);
        }
        let mut prev = ctx.root;
        for parsed in &ctx.nodes {
            if parsed.range.start <= cursor && cursor <= parsed.range.end {
                return (prev, parsed.range.start, ctx);
            }
            prev = parsed.node;
        }
        return (prev, ctx.range.start, ctx);
    }
    (ctx.root, ctx.range.start, ctx)
}
