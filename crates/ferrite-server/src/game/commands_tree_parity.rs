//! Tree-parity test: walks the live server tree (`dispatcher()`) through the public
//! `CommandDispatcher` API and diffs it, path by path, against the vanilla 26.3
//! `commands.json` report. Redirecting nodes are leaves on both sides.
//!
//! A difference is not automatically a gap. Every accepted difference is listed in
//! [`ALLOWED`] with its classification and reason (see `docs/COMMANDS-STATUS.md`); a new
//! difference, or an allow-list entry that no longer occurs, fails the test.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ferrite_command::{CommandDispatcher, NodeId, NodeKind};
use serde_json::Value;

use super::commands::{Source, dispatcher};

/// `(root, difference glob, class, reason)`. `*` in the glob matches any text; the root `*`
/// matches every root. Classes: `a` behaviour-equivalent layout, `c` cosmetic or an
/// extra-lenient extension, `x` a real gap with a written blocker in `docs/COMMANDS-STATUS.md`,
/// `o` owned by another lane (reported to it, not fixed here).
pub(super) const ALLOWED: &[(&str, &str, char, &str)] = &[
    // Parser substitutions: the text accepted is the same, validation happens at run time.
    (
        "*",
        "*: parser vanilla=minecraft:resource_key ferrite=minecraft:resource_location",
        'c',
        "registry-key arguments parse as resource_location and are validated when the command runs",
    ),
    (
        "*",
        "*: parser vanilla=minecraft:resource ferrite=minecraft:resource_location",
        'c',
        "registry-entry arguments parse as resource_location and are validated when the command runs",
    ),
    (
        "locate",
        "locate structure structure: parser vanilla=minecraft:resource_or_tag_key ferrite=minecraft:resource_or_tag",
        'c',
        "same text (`#tag` or id); the key/non-key distinction only matters for the client's suggestions",
    ),
    (
        "execute",
        "*: parser vanilla=minecraft:loot_predicate ferrite=minecraft:resource_location",
        'c',
        "a predicate id; the argument is resolved against datapack predicates when the command runs",
    ),
    (
        "execute",
        "*: parser vanilla=minecraft:objective ferrite=brigadier:string",
        'c',
        "an objective name; existence is checked when the command runs (`check_objective`)",
    ),
    (
        "execute",
        "execute align axes: parser vanilla=minecraft:swizzle ferrite=brigadier:string",
        'a',
        "a string parsed as the axes xyz subset by the align handler (`Unknown axis` otherwise)",
    ),
    (
        "schedule",
        "schedule clear function: parser vanilla=minecraft:resource_location ferrite=minecraft:function",
        'c',
        "ferrite also accepts function tags here; vanilla's id is a subset",
    ),
    // /execute layout.
    (
        "execute",
        "execute run: redirect vanilla=false ferrite=true",
        'a',
        "vanilla's `run` re-dispatches from the root through `ExecuteCommand`'s own tree; here it is a redirect to the root",
    ),
    (
        "execute",
        "execute * function name: executable vanilla=false ferrite=true",
        'c',
        "`if|unless function <name>` ends the chain and reports the test; vanilla needs `run`",
    ),
    (
        "execute",
        "execute * score target targetObjective *: only in *",
        'a',
        "the five comparison literals `< <= = > >=` are one `operation` argument with the same meaning",
    ),
    (
        "execute",
        "execute store * * path *: only in vanilla",
        'a',
        "the byte/short/int/long/float/double literals are one `type` word plus the scale argument",
    ),
    (
        "execute",
        "execute store * * path type: only in ferrite",
        'a',
        "the byte/short/int/long/float/double literals are one `type` word plus the scale argument",
    ),
    // /data.
    // /team: vanilla takes a `color` argument (and `reset`), ferrite lists the literals.
    (
        "team",
        "team modify team color *: only in ferrite",
        'a',
        "one literal per colour instead of vanilla's single `value` colour argument",
    ),
    (
        "team",
        "team modify team color value: only in vanilla",
        'a',
        "one literal per colour instead of vanilla's single `value` colour argument",
    ),
    // /gamerule: ferrite also registers the pre-26 camelCase names.
    (
        "gamerule",
        "gamerule *: only in ferrite",
        'c',
        "legacy camelCase gamerule names are accepted as aliases (vanilla 26.3 only has the snake_case ids)",
    ),
    (
        "return",
        "return run command: only in ferrite",
        'a',
        "`return run` carries a redirect to the root; vanilla re-dispatches the same way",
    ),
    // Non-vanilla roots.
    (
        "mspt",
        "mspt: root only in ferrite",
        'c',
        "Paper/Folia command",
    ),
    (
        "tps",
        "tps: root only in ferrite",
        'c',
        "Paper/Folia command",
    ),
    (
        "profile",
        "profile: root only in ferrite",
        'c',
        "Spark-style profiling command",
    ),
    // Other lanes.
    (
        "item",
        "item * entity target: only in vanilla",
        'o',
        "argument name `target` vs `targets` (lane A/B item)",
    ),
    (
        "item",
        "item * entity targets: only in ferrite",
        'o',
        "argument name `target` vs `targets` (lane A/B item)",
    ),
    (
        "place",
        "place template template: executable vanilla=true ferrite=false",
        'o',
        "/place belongs to lane B",
    ),
    (
        "place",
        "place * *: parser vanilla=minecraft:feature ferrite=minecraft:resource_location",
        'o',
        "/place belongs to lane B",
    ),
    (
        "fillbiome",
        "fillbiome from to biome: parser vanilla=minecraft:resource ferrite=minecraft:resource_or_tag",
        'o',
        "/fillbiome belongs to lane B",
    ),
];

/// `*` matches any run of characters.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| glob(rest, &text[skip..])),
        Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

/// Differences that match no allow-list entry, and entries that matched nothing.
pub(super) fn unexplained(all: &BTreeMap<String, Vec<String>>) -> (Vec<String>, Vec<&'static str>) {
    let mut used = vec![false; ALLOWED.len()];
    let mut bad = Vec::new();
    for (root, diffs) in all {
        for diff in diffs {
            let hit = ALLOWED.iter().position(|(r, g, _, _)| {
                (*r == "*" || r == root) && glob(g.as_bytes(), diff.as_bytes())
            });
            match hit {
                Some(i) => used[i] = true,
                None => bad.push(format!("[{root}] {diff}")),
            }
        }
    }
    let stale = ALLOWED
        .iter()
        .zip(used)
        .filter(|(_, used)| !used)
        .map(|((_, glob, _, _), _)| *glob)
        .collect();
    (bad, stale)
}

fn vanilla() -> Option<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vanilla/26.3/generated/reports/commands.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn ferrite_children(tree: &CommandDispatcher<Source>, id: NodeId) -> BTreeMap<String, NodeId> {
    tree.node(id)
        .children()
        .iter()
        .map(|c| (tree.node(*c).name().to_owned(), *c))
        .collect()
}

fn walk(
    tree: &CommandDispatcher<Source>,
    id: NodeId,
    vanilla: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    let node = tree.node(id);
    let v_exec = vanilla
        .get("executable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if v_exec != node.is_executable() {
        out.push(format!(
            "{path}: executable vanilla={v_exec} ferrite={}",
            node.is_executable()
        ));
    }
    let v_redirect = vanilla.get("redirect").is_some();
    if v_redirect != node.redirect().is_some() {
        out.push(format!(
            "{path}: redirect vanilla={v_redirect} ferrite={}",
            node.redirect().is_some()
        ));
    }
    if v_redirect || node.redirect().is_some() {
        return;
    }
    let ours = ferrite_children(tree, id);
    let empty = serde_json::Map::new();
    let theirs = vanilla
        .get("children")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let names: BTreeSet<&str> = ours
        .keys()
        .map(String::as_str)
        .chain(theirs.keys().map(String::as_str))
        .collect();
    for name in names {
        let child_path = format!("{path} {name}");
        match (ours.get(name), theirs.get(name)) {
            (Some(o), Some(t)) => {
                if let NodeKind::Argument { ty, .. } = tree.node(*o).kind() {
                    let v_parser = t.get("parser").and_then(Value::as_str).unwrap_or("?");
                    if v_parser != ty.parser_id() {
                        out.push(format!(
                            "{child_path}: parser vanilla={v_parser} ferrite={}",
                            ty.parser_id()
                        ));
                    }
                }
                walk(tree, *o, t, &child_path, out);
            }
            (Some(_), None) => out.push(format!("{child_path}: only in ferrite")),
            (None, Some(_)) => out.push(format!("{child_path}: only in vanilla")),
            (None, None) => {}
        }
    }
}

/// Differences of one root, or `None` when the root is missing on either side.
fn diff_root(tree: &CommandDispatcher<Source>, vanilla: &Value, root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let ours = tree
        .root_literals()
        .into_iter()
        .find(|(n, _)| *n == root)
        .map(|(_, id)| id);
    let theirs = vanilla["children"].get(root);
    match (ours, theirs) {
        (Some(o), Some(t)) => walk(tree, o, t, root, &mut out),
        (None, Some(_)) => out.push(format!("{root}: root only in vanilla")),
        (Some(_), None) => out.push(format!("{root}: root only in ferrite")),
        (None, None) => {}
    }
    out
}

/// Every difference of the live tree, keyed by root.
pub(super) fn all_diffs() -> Option<BTreeMap<String, Vec<String>>> {
    let vanilla = vanilla()?;
    let tree = dispatcher();
    let mut roots: BTreeSet<String> = tree
        .root_literals()
        .into_iter()
        .map(|(n, _)| n.to_owned())
        .collect();
    roots.extend(vanilla["children"].as_object().unwrap().keys().cloned());
    let mut all = BTreeMap::new();
    for root in roots {
        let d = diff_root(&tree, &vanilla, &root);
        if !d.is_empty() {
            all.insert(root, d);
        }
    }
    Some(all)
}

/// The live tree equals vanilla's 26.3 `commands.json` except for the allow-listed
/// differences, and the allow-list has no stale entries.
#[test]
fn tree_matches_vanilla_except_the_allow_list() {
    let Some(all) = all_diffs() else { return };
    if std::env::var_os("FERRITE_TREE_DUMP").is_some() {
        for (root, diffs) in &all {
            for d in diffs {
                println!("DIFF [{root}] {d}");
            }
        }
        println!(
            "TREE roots_with_diffs={} allow_list={}",
            all.len(),
            ALLOWED.len()
        );
    }
    let (bad, stale) = unexplained(&all);
    assert!(
        bad.is_empty(),
        "differences from vanilla's command tree not in the allow-list:\n{}",
        bad.join("\n")
    );
    assert!(stale.is_empty(), "stale allow-list entries: {stale:#?}");
}

#[test]
fn allow_list_globs_behave() {
    assert!(glob(b"a*c", b"abbc"));
    assert!(glob(b"*", b""));
    assert!(!glob(b"a*c", b"abbd"));
}
