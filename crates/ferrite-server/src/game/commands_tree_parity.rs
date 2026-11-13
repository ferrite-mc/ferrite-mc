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

/// `(root, difference)` pairs: the difference text is produced by [`diff_root`].
/// Classes: `a` behaviour-equivalent layout, `c` cosmetic.
pub(super) const ALLOWED: &[(&str, &str, &str)] = &[];

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

#[test]
fn dump_tree_diffs() {
    let Some(all) = all_diffs() else { return };
    if std::env::var_os("FERRITE_TREE_DUMP").is_some() {
        for (root, diffs) in &all {
            for d in diffs {
                println!("DIFF [{root}] {d}");
            }
        }
    }
}
