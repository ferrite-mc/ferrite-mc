//! Turns the vanilla data generator output in `vanilla/<version>/generated/` into static Rust
//! tables for block states, registries, packet ids and block physics.
//!
//! The data is produced by `python tools/fetch_vanilla.py`; this script only reads it and
//! fails with a pointer to that command when it is missing.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Minecraft version whose generated data is compiled in.
const VERSION: &str = "26.3";

/// Line width the generated tables are wrapped at (they are not run through rustfmt).
const WIDTH: usize = 100;

fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for build scripts"),
    );
    // crates/ferrite-data -> crates -> <workspace>
    let workspace = manifest_dir
        .ancestors()
        .nth(2)
        .expect("ferrite-data lives in <workspace>/crates/ferrite-data")
        .to_path_buf();
    let generated = workspace.join("vanilla").join(VERSION).join("generated");

    let blocks = generated.join("reports").join("blocks.json");
    let registries = generated.join("reports").join("registries.json");
    let packets = generated.join("reports").join("packets.json");
    let datapack = generated.join("reports").join("datapack.json");
    let data = generated.join("data").join("minecraft");
    let tags = data.join("tags");
    let physics = generated.join("ferrite").join("block_states.json");
    let piston_states = generated.join("ferrite").join("piston_states.json");
    // Optional: an older extraction may predate entity types, so a missing file only warns.
    let entity_types = generated.join("ferrite").join("entity_types.json");
    for path in [
        &blocks,
        &registries,
        &packets,
        &datapack,
        &data,
        &tags,
        &physics,
        &piston_states,
    ] {
        if !path.exists() {
            missing(path);
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", entity_types.display());
    println!(
        "cargo:rerun-if-changed={}",
        generated.join("ferrite").display()
    );
    for path in [
        &blocks,
        &registries,
        &packets,
        &datapack,
        &data,
        &tags,
        &physics,
        &piston_states,
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set for build scripts"));
    write_file(&out_dir.join("blocks.rs"), &gen_blocks(&blocks));
    write_file(
        &out_dir.join("registries.rs"),
        &gen_registries(&registries, &data),
    );
    write_file(&out_dir.join("packets.rs"), &gen_packets(&packets));
    write_file(&out_dir.join("tags.rs"), &gen_tags(&tags, &datapack));
    write_file(&out_dir.join("block_props.rs"), &gen_block_props(&physics));
    write_file(
        &out_dir.join("piston_props.rs"),
        &gen_piston_props(&piston_states, &physics),
    );
    write_file(
        &out_dir.join("entity_props.rs"),
        &gen_entity_props(&entity_types),
    );
    let trades = data.join("villager_trade");
    let trade_sets = data.join("trade_set");
    for path in [&trades, &trade_sets, &tags.join("villager_trade")] {
        if !path.exists() {
            missing(path);
        }
    }
    write_file(
        &out_dir.join("villager_trades.rs"),
        &gen_villager_trades(
            &registries,
            &trades,
            &trade_sets,
            &tags.join("villager_trade"),
        ),
    );
}

/// Reports missing vanilla data, telling the user how to produce it.
fn missing(path: &Path) -> ! {
    panic!(
        "vanilla data for Minecraft {VERSION} is missing at {}\n\
         run `python tools/fetch_vanilla.py` in the workspace root to generate it",
        path.display()
    )
}

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
}

fn gen_piston_props(path: &Path, physics: &Path) -> String {
    let data = read_json(path);
    let states = data.as_array().expect("piston states must be an array");
    let physics_data = read_json(physics);
    let expected = physics_data["states"]
        .as_array()
        .expect("physics states must be an array")
        .len();
    assert_eq!(
        states.len(),
        expected,
        "piston metadata does not match the block state report; rerun the vanilla extractor"
    );
    let mut result = header("ferrite/piston_states.json");
    result.push_str("static PISTON_STATES: &[u8] = &[\n");
    let values: Vec<String> = states
        .iter()
        .map(|state| {
            let reaction = match state["reaction"].as_str().expect("piston reaction") {
                "PUSH_PULL" => 0,
                "PUSH" => 1,
                "POPPED" => 2,
                "IMMOVEABLE" => 3,
                "IGNORE_ENTITY" => 4,
                other => panic!("unknown piston reaction {other}"),
            };
            let entity = state["blockEntity"].as_bool().expect("block entity flag");
            (reaction | if entity { 8 } else { 0 }).to_string()
        })
        .collect();
    items(&mut result, "    ", &values);
    result.push_str("\n];\n");
    result
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()))
}

fn header(report: &str) -> String {
    format!(
        "// @generated by crates/ferrite-data/build.rs from vanilla/{VERSION}/generated/{report}.\n\
         // Do not edit; rerun the build (or `python tools/fetch_vanilla.py`) instead.\n\n"
    )
}

/// Renders a quoted Rust string literal.
fn literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                write!(out, "\\u{{{:x}}}", ch as u32).expect("writing to a String cannot fail");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Appends a wrapped, comma separated list of already rendered items.
fn items(out: &mut String, indent: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push_str(indent);
    let mut width = indent.len();
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
            if width + 2 + value.len() > WIDTH {
                out.push('\n');
                out.push_str(indent);
                width = indent.len();
            } else {
                out.push(' ');
                width += 1;
            }
        }
        out.push_str(value);
        width += value.len();
    }
    out.push(',');
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| literal(value)).collect()
}

// ---------------------------------------------------------------------------------------------
// blocks
// ---------------------------------------------------------------------------------------------

/// One block property: its name, its values (sorted) and the value used by the default state.
struct Prop {
    name: String,
    values: Vec<String>,
    default: u16,
}

/// The states of a block, panicking with the block name when the report is malformed.
fn states_of<'a>(name: &str, block: &'a serde_json::Value) -> &'a [serde_json::Value] {
    block
        .get("states")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{name}: expected a states array"))
        .as_slice()
}

/// The global state id of a state, checked to fit in a `u16`.
fn state_id_of(name: &str, state: &serde_json::Value) -> u16 {
    let id = state
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("{name}: state without an id"));
    u16::try_from(id).unwrap_or_else(|_| panic!("{name}: state id {id} does not fit in u16"))
}

fn gen_blocks(path: &Path) -> String {
    let report = read_json(path);
    let blocks = report
        .as_object()
        .unwrap_or_else(|| panic!("{}: expected an object of blocks", path.display()));
    let mut names: Vec<&str> = blocks.keys().map(String::as_str).collect();
    names.sort_unstable();

    // State ids are contiguous, so dense `id -> ...` tables are exact; checked below.
    let mut state_count = 0;
    for name in &names {
        for state in states_of(name, &blocks[*name]) {
            state_count = state_count.max(state_id_of(name, state) as usize + 1);
        }
    }
    let mut state_blocks: Vec<Option<u16>> = vec![None; state_count];
    let mut state_prop_sets: Vec<Option<u16>> = vec![None; state_count];
    let mut prop_sets: Vec<Vec<(String, String)>> = Vec::new();
    let mut prop_set_ids: BTreeMap<String, usize> = BTreeMap::new();

    let mut out = header("reports/blocks.json");
    writeln!(
        out,
        "/// Number of block states reported by the vanilla data generator.\n\
         pub const STATE_COUNT: usize = {state_count};\n"
    )
    .expect("writing to a String cannot fail");
    out.push_str("/// Block definitions, sorted by `name` for binary search.\n");
    out.push_str("static BLOCKS: &[Block] = &[\n");

    for (index, name) in names.iter().enumerate() {
        let block = &blocks[*name];
        let block_index = u16::try_from(index)
            .unwrap_or_else(|_| panic!("{name}: more than 65535 blocks in blocks.json"));
        let properties = block
            .get("properties")
            .and_then(serde_json::Value::as_object);
        let mut prop_names: Vec<&str> = properties
            .map(|properties| properties.keys().map(String::as_str).collect())
            .unwrap_or_default();
        prop_names.sort_unstable();

        let mut props: Vec<Prop> = prop_names
            .iter()
            .map(|prop| {
                let values = properties
                    .and_then(|properties| properties.get(*prop))
                    .and_then(serde_json::Value::as_array)
                    .unwrap_or_else(|| panic!("{name}: property {prop} has no value list"));
                let mut values: Vec<&str> = values
                    .iter()
                    .map(|value| {
                        value.as_str().unwrap_or_else(|| {
                            panic!("{name}: property {prop} has a non-string value")
                        })
                    })
                    .collect();
                values.sort_unstable();
                Prop {
                    name: (*prop).to_string(),
                    values: values.into_iter().map(str::to_string).collect(),
                    default: 0,
                }
            })
            .collect();

        // Mixed radix index over the (sorted) properties; the global state id is `slots[index]`.
        let total: usize = props
            .iter()
            .map(|prop| prop.values.len())
            .product::<usize>()
            .max(1);
        let mut slots: Vec<Option<u16>> = vec![None; total];
        let mut has_default = false;

        for state in states_of(name, block) {
            let id = state_id_of(name, state);
            let is_default = state
                .get("default")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            has_default |= is_default;
            let values = state
                .get("properties")
                .and_then(serde_json::Value::as_object);

            let mut canonical = 0;
            let mut stride = total;
            for prop in props.iter_mut() {
                stride /= prop.values.len();
                let value = values
                    .and_then(|values| values.get(&prop.name))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| {
                        panic!("{name}: state {id} has no value for property {}", prop.name)
                    });
                let position = prop
                    .values
                    .binary_search_by(|candidate| candidate.as_str().cmp(value))
                    .unwrap_or_else(|_| {
                        panic!("{name}: unknown value {value:?} for property {}", prop.name)
                    });
                canonical += position * stride;
                if is_default {
                    prop.default = u16::try_from(position).unwrap_or_else(|_| {
                        panic!("{name}: property {} has too many values", prop.name)
                    });
                }
            }

            if slots[canonical].replace(id).is_some() {
                panic!("{name}: two states share one property combination");
            }

            // Remember the state's property list, shared between states with equal values.
            let list: Vec<(String, String)> = props
                .iter()
                .map(|prop| {
                    let value = values
                        .and_then(|values| values.get(&prop.name))
                        .and_then(serde_json::Value::as_str)
                        .expect("state property values were validated above");
                    (prop.name.clone(), value.to_string())
                })
                .collect();
            let key = list
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("\u{1f}");
            let set = *prop_set_ids.entry(key).or_insert_with(|| {
                prop_sets.push(list);
                prop_sets.len() - 1
            });
            let set = u16::try_from(set)
                .unwrap_or_else(|_| panic!("more than 65535 distinct property lists"));

            let id = id as usize;
            state_blocks[id] = Some(block_index);
            state_prop_sets[id] = Some(set);
        }

        if !has_default {
            panic!("{name}: block has no default state");
        }
        if slots.iter().any(Option::is_none) {
            panic!("{name}: states do not cover every property combination");
        }

        writeln!(out, "    Block {{").expect("writing to a String cannot fail");
        writeln!(out, "        name: {},", literal(name)).expect("writing to a String cannot fail");
        if props.is_empty() {
            out.push_str("        props: &[],\n");
        } else {
            out.push_str("        props: &[\n");
            for prop in &props {
                writeln!(
                    out,
                    "            Prop {{ name: {}, values: &[{}], default: {} }},",
                    literal(&prop.name),
                    strings(&prop.values.iter().map(String::as_str).collect::<Vec<_>>()).join(", "),
                    prop.default,
                )
                .expect("writing to a String cannot fail");
            }
            out.push_str("        ],\n");
        }
        out.push_str("        map: &[\n");
        let map: Vec<String> = slots
            .iter()
            .map(|slot| slot.expect("checked above").to_string())
            .collect();
        items(&mut out, "            ", &map);
        out.push_str("\n        ],\n");
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");

    // Deduplicated property lists, one entry per distinct state property set.
    out.push_str("/// Distinct block state property lists, in no particular order.\n");
    out.push_str("static PROP_SETS: &[&[(&str, &str)]] = &[\n");
    for set in &prop_sets {
        let values: Vec<String> = set
            .iter()
            .map(|(key, value)| format!("({}, {})", literal(key), literal(value)))
            .collect();
        out.push_str("    &[");
        out.push_str(&values.join(", "));
        out.push_str("],\n");
    }
    out.push_str("];\n\n");

    out.push_str("/// State id -> index into `BLOCKS`.\n");
    out.push_str("static STATE_BLOCK: &[u16] = &[\n");
    let rows: Vec<String> = state_blocks
        .iter()
        .map(|block| {
            block
                .unwrap_or_else(|| panic!("blocks.json state ids are not contiguous"))
                .to_string()
        })
        .collect();
    items(&mut out, "    ", &rows);
    out.push_str("\n];\n\n");

    out.push_str("/// State id -> index into `PROP_SETS`.\n");
    out.push_str("static STATE_PROP_SET: &[u16] = &[\n");
    let rows: Vec<String> = state_prop_sets
        .iter()
        .map(|set| {
            set.unwrap_or_else(|| panic!("blocks.json state ids are not contiguous"))
                .to_string()
        })
        .collect();
    items(&mut out, "    ", &rows);
    out.push_str("\n];\n");

    out
}

// ---------------------------------------------------------------------------------------------
// registries
// ---------------------------------------------------------------------------------------------

fn gen_registries(path: &Path, data_root: &Path) -> String {
    let report = read_json(path);
    let registries = report
        .as_object()
        .unwrap_or_else(|| panic!("{}: expected an object of registries", path.display()));
    let mut names: Vec<&str> = registries.keys().map(String::as_str).collect();
    names.sort_unstable();

    let mut out = header("reports/registries.json");
    out.push_str("/// Built-in registries, sorted by `name` for binary search.\n");
    out.push_str("static REGISTRIES: &[Registry] = &[\n");
    for name in names {
        let entries = registries[name]
            .get("entries")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("{name}: registry without entries"));
        let mut entries: Vec<(&str, i32)> = entries
            .iter()
            .map(|(entry, value)| {
                let id = value
                    .get("protocol_id")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_else(|| panic!("{name}: entry {entry} has no protocol_id"));
                (
                    entry.as_str(),
                    i32::try_from(id).unwrap_or_else(|_| {
                        panic!("{name}: entry {entry} protocol id {id} does not fit in i32")
                    }),
                )
            })
            .collect();
        entries.sort_unstable_by(|left, right| left.0.cmp(right.0));

        writeln!(out, "    Registry {{").expect("writing to a String cannot fail");
        writeln!(out, "        name: {},", literal(name)).expect("writing to a String cannot fail");
        out.push_str("        names: &[\n");
        items(
            &mut out,
            "            ",
            &strings(&entries.iter().map(|(entry, _)| *entry).collect::<Vec<_>>()),
        );
        out.push_str("\n        ],\n");
        out.push_str("        ids: &[\n");
        items(
            &mut out,
            "            ",
            &entries
                .iter()
                .map(|(_, id)| id.to_string())
                .collect::<Vec<_>>(),
        );
        out.push_str("\n        ],\n");
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");

    // Data-driven registries: `data/minecraft/<registry>/**/*.json`.
    let mut files: Vec<(String, String)> = Vec::new();
    walk(data_root, "", &mut files);
    let roots: BTreeSet<&str> = files.iter().map(|(dir, _)| dir.as_str()).collect();
    let mut data: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (dir, stem) in &files {
        let Some(registry) = registry_root(dir, &roots) else {
            continue;
        };
        let suffix = dir[registry.len()..].trim_start_matches('/');
        let entry = if suffix.is_empty() {
            format!("minecraft:{stem}")
        } else {
            format!("minecraft:{suffix}/{stem}")
        };
        data.entry(registry).or_default().push(entry);
    }
    for entries in data.values_mut() {
        entries.sort_unstable();
        entries.dedup();
    }

    out.push_str(
        "/// Data-driven registries: relative path under `data/minecraft` and its entries.\n",
    );
    out.push_str("static DATA: &[(&str, &[&str])] = &[\n");
    for (registry, entries) in &data {
        writeln!(out, "    ({}, &[", literal(registry)).expect("writing to a String cannot fail");
        items(
            &mut out,
            "        ",
            &strings(&entries.iter().map(String::as_str).collect::<Vec<_>>()),
        );
        out.push_str("\n    ]),");
    }
    out.push_str("\n];\n");

    out
}

/// Collects `(directory, file stem)` for every JSON file below `root`, skipping the bundled
/// datapack resources and the tag overlays (neither holds registry entries).
fn walk(root: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    let dir = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("failed to list {}: {err}", dir.display()));
    let mut children: Vec<(bool, String)> = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("failed to list {}: {err}", dir.display()));
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to stat {}: {err}", entry.path().display()))
            .is_dir();
        children.push((is_dir, name));
    }
    children.sort();
    for (is_dir, name) in children {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if is_dir {
            if prefix.is_empty() && (name == "datapacks" || name == "tags") {
                continue;
            }
            walk(root, &relative, out);
        } else if let Some(stem) = name.strip_suffix(".json") {
            out.push((prefix.to_string(), stem.to_string()));
        }
    }
}

/// The registry a data file in `dir` belongs to: the shallowest ancestor directory (including
/// `dir` itself) that directly holds JSON files, so `worldgen/biome/plains.json` belongs to
/// `worldgen/biome` while `worldgen/structure/village/plains/town_centers/x.json` keeps its
/// sub path as part of the entry name.
fn registry_root<'a>(dir: &'a str, roots: &BTreeSet<&str>) -> Option<&'a str> {
    // Shallowest ancestor directory that directly holds JSON files, `dir` itself included.
    let mut prefix = dir.len();
    for (offset, ch) in dir.char_indices() {
        if ch == '/' && roots.contains(&dir[..offset]) {
            prefix = offset;
            break;
        }
    }
    roots.contains(&dir[..prefix]).then(|| &dir[..prefix])
}

// ---------------------------------------------------------------------------------------------
// tags
// ---------------------------------------------------------------------------------------------

/// One value of a tag file: a concrete registry entry or a reference to another tag.
enum TagEntry {
    Entry(String),
    /// A tag reference; `required: false` may resolve to nothing without failing the build.
    Tag {
        name: String,
        required: bool,
    },
}

/// A tag after resolution: its sorted, deduplicated entries and how many entries its expanded
/// (not deduplicated) value list holds.
struct ResolvedTag {
    entries: Vec<String>,
    occurrences: usize,
}

fn gen_tags(tags_root: &Path, datapack: &Path) -> String {
    let registries = tagged_registries(datapack);
    let mut files = Vec::new();
    tag_files(tags_root, "", &mut files);
    files.sort();

    // Tags keyed by (registry path, namespaced tag name) so the same tag name in two registries
    // stays distinct.
    let mut raw: BTreeMap<(String, String), Vec<TagEntry>> = BTreeMap::new();
    for (relative, path) in &files {
        let Some((registry, tag)) = split_tag_path(relative, &registries) else {
            panic!(
                "{}: tag is not below a registry that supports tags",
                path.display()
            );
        };
        let value = read_json(path);
        let values = value
            .get("values")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("{}: tag file without a values array", path.display()));
        let mut entries = Vec::with_capacity(values.len());
        for value in values {
            let (id, required) = match value {
                serde_json::Value::String(id) => (id.as_str(), true),
                serde_json::Value::Object(object) => {
                    let id = object
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_else(|| panic!("{}: tag entry without an id", path.display()));
                    let required = object
                        .get("required")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    (id, required)
                }
                _ => panic!(
                    "{}: a tag entry must be a string or an object",
                    path.display()
                ),
            };
            entries.push(tag_entry(id, required, path));
        }
        let key = (registry.to_string(), format!("minecraft:{tag}"));
        if raw.insert(key, entries).is_some() {
            panic!("{}: duplicate tag file", path.display());
        }
    }

    let resolved = resolve_tags(&raw);
    let registries: BTreeSet<&str> = raw.keys().map(|(registry, _)| registry.as_str()).collect();
    let occurrences: usize = resolved.values().map(|tag| tag.occurrences).sum();

    let mut out = header("data/minecraft/tags");
    writeln!(
        out,
        "/// Number of registries with built-in tags.\npub const REGISTRY_COUNT: usize = {};\n",
        registries.len()
    )
    .expect("writing to a String cannot fail");
    writeln!(
        out,
        "/// Number of built-in tags.\npub const TAG_COUNT: usize = {};\n",
        resolved.len()
    )
    .expect("writing to a String cannot fail");
    writeln!(
        out,
        "/// Total resolved entries of all tags: an entry reached through several `#tag`\n\
         /// references counts once per reference. This is the size of the expanded value lists\n\
         /// before duplicates are removed.\npub const RESOLVED_ENTRIES: usize = {occurrences};\n"
    )
    .expect("writing to a String cannot fail");

    out.push_str(
        "/// Registries with built-in tags, sorted by name.\nstatic REGISTRIES: &[&str] = &[\n",
    );
    let names: Vec<String> = registries
        .iter()
        .map(|registry| literal(&format!("minecraft:{registry}")))
        .collect();
    items(&mut out, "    ", &names);
    out.push_str("\n];\n\n");

    out.push_str(
        "/// Built-in tags, sorted by registry and then name.\nstatic TAGS: &[Tag] = &[\n",
    );
    for ((registry, name), tag) in &resolved {
        out.push_str("    Tag {\n");
        writeln!(
            out,
            "        registry: {},",
            literal(&format!("minecraft:{registry}"))
        )
        .expect("writing to a String cannot fail");
        writeln!(out, "        name: {},", literal(name)).expect("writing to a String cannot fail");
        if tag.entries.is_empty() {
            out.push_str("        entries: &[],\n");
        } else {
            out.push_str("        entries: &[\n");
            items(
                &mut out,
                "            ",
                &tag.entries
                    .iter()
                    .map(String::as_str)
                    .map(literal)
                    .collect::<Vec<_>>(),
            );
            out.push_str("\n        ],\n");
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n");

    out
}

/// Names of the registries that support tags, from `reports/datapack.json`.
fn tagged_registries(path: &Path) -> BTreeSet<String> {
    let report = read_json(path);
    let registries = report
        .get("registries")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("{}: expected a registries object", path.display()));
    registries
        .iter()
        .filter(|(_, value)| {
            value
                .get("tags")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .map(|(name, _)| name.strip_prefix("minecraft:").unwrap_or(name).to_string())
        .collect()
}

/// One tag file's value: an entry id (namespaced if needed) or a `#tag` reference.
fn tag_entry(id: &str, required: bool, path: &Path) -> TagEntry {
    match id.strip_prefix('#') {
        None => TagEntry::Entry(namespaced(id)),
        Some("") => panic!("{}: empty tag reference", path.display()),
        Some(reference) => TagEntry::Tag {
            name: namespaced(reference),
            required,
        },
    }
}

/// Adds the `minecraft:` namespace to a name that does not carry one.
fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

/// Collects `(path without .json, file)` for every tag file below `root`.
fn tag_files(root: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let dir = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("failed to list {}: {err}", dir.display()));
    let mut children: Vec<(bool, String)> = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("failed to list {}: {err}", dir.display()));
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to stat {}: {err}", entry.path().display()))
            .is_dir();
        children.push((is_dir, name));
    }
    children.sort();
    for (is_dir, name) in children {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if is_dir {
            tag_files(root, &relative, out);
        } else if let Some(stem) = name.strip_suffix(".json") {
            let relative = if prefix.is_empty() {
                stem.to_string()
            } else {
                format!("{prefix}/{stem}")
            };
            out.push((relative, dir.join(name)));
        }
    }
}

/// Splits a tag path into the deepest registry directory and the tag name below it, e.g.
/// `worldgen/biome/is_beach` -> `("worldgen/biome", "is_beach")`.
fn split_tag_path<'a>(
    relative: &'a str,
    registries: &BTreeSet<String>,
) -> Option<(&'a str, &'a str)> {
    for (offset, ch) in relative.char_indices().rev() {
        if ch == '/' {
            let registry = &relative[..offset];
            if registries.contains(registry) {
                return Some((registry, &relative[offset + 1..]));
            }
        }
    }
    None
}

/// Resolves every tag, expanding `#tag` references within the same registry.
fn resolve_tags(
    raw: &BTreeMap<(String, String), Vec<TagEntry>>,
) -> BTreeMap<(String, String), ResolvedTag> {
    let mut resolved = BTreeMap::new();
    let mut state = BTreeMap::new();
    let keys: Vec<(String, String)> = raw.keys().cloned().collect();
    for key in &keys {
        resolve_tag(key, raw, &mut state, &mut resolved);
    }
    resolved
}

/// Resolves one tag, memoizing the result; `state` is 1 while a tag is being expanded and 2 once
/// it is done, which detects cyclic references.
fn resolve_tag(
    key: &(String, String),
    raw: &BTreeMap<(String, String), Vec<TagEntry>>,
    state: &mut BTreeMap<(String, String), u8>,
    resolved: &mut BTreeMap<(String, String), ResolvedTag>,
) -> ResolvedTag {
    if let Some(tag) = resolved.get(key) {
        return ResolvedTag {
            entries: tag.entries.clone(),
            occurrences: tag.occurrences,
        };
    }
    match state.get(key).copied() {
        Some(1) => panic!("tag #{} is part of a reference cycle", key.1),
        Some(2) => unreachable!("resolved tags are memoized"),
        _ => {}
    }
    state.insert(key.clone(), 1);

    let mut entries = BTreeSet::new();
    let mut occurrences = 0;
    for entry in raw.get(key).expect("tags are resolved from the raw table") {
        match entry {
            TagEntry::Entry(id) => {
                entries.insert(id.clone());
                occurrences += 1;
            }
            TagEntry::Tag { name, required } => {
                let reference = (key.0.clone(), name.clone());
                if raw.contains_key(&reference) {
                    let referenced = resolve_tag(&reference, raw, state, resolved);
                    entries.extend(referenced.entries);
                    occurrences += referenced.occurrences;
                } else if *required {
                    panic!("tag #{} references missing tag #{name}", key.1);
                }
            }
        }
    }

    state.insert(key.clone(), 2);
    let tag = ResolvedTag {
        entries: entries.into_iter().collect(),
        occurrences,
    };
    resolved.insert(
        key.clone(),
        ResolvedTag {
            entries: tag.entries.clone(),
            occurrences: tag.occurrences,
        },
    );
    tag
}

// ---------------------------------------------------------------------------------------------
// packets
// ---------------------------------------------------------------------------------------------

fn gen_packets(path: &Path) -> String {
    let report = read_json(path);
    let states = report
        .as_object()
        .unwrap_or_else(|| panic!("{}: expected an object of states", path.display()));
    let mut state_names: Vec<&str> = states.keys().map(String::as_str).collect();
    state_names.sort_unstable();

    let mut out = header("reports/packets.json");
    out.push_str("/// `(state, bound)` tables, sorted by name for binary search.\n");
    out.push_str("static TABLES: &[Table] = &[\n");
    for state in state_names {
        let bounds = states[state]
            .as_object()
            .unwrap_or_else(|| panic!("{state}: expected an object of directions"));
        let mut bound_names: Vec<&str> = bounds.keys().map(String::as_str).collect();
        bound_names.sort_unstable();
        for bound in bound_names {
            let packets = bounds[bound]
                .as_object()
                .unwrap_or_else(|| panic!("{state}/{bound}: expected an object of packets"));
            let mut packets: Vec<(&str, i32)> = packets
                .iter()
                .map(|(name, value)| {
                    let id = value
                        .get("protocol_id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_else(|| panic!("{state}/{bound}/{name}: no protocol_id"));
                    (
                        name.as_str(),
                        i32::try_from(id).unwrap_or_else(|_| {
                            panic!("{state}/{bound}/{name}: protocol id {id} does not fit in i32")
                        }),
                    )
                })
                .collect();
            packets.sort_unstable_by(|left, right| left.0.cmp(right.0));

            writeln!(out, "    Table {{").expect("writing to a String cannot fail");
            writeln!(out, "        state: {},", literal(state))
                .expect("writing to a String cannot fail");
            writeln!(out, "        bound: {},", literal(bound))
                .expect("writing to a String cannot fail");
            out.push_str("        names: &[\n");
            items(
                &mut out,
                "            ",
                &strings(&packets.iter().map(|(name, _)| *name).collect::<Vec<_>>()),
            );
            out.push_str("\n        ],\n");
            out.push_str("        ids: &[\n");
            items(
                &mut out,
                "            ",
                &packets
                    .iter()
                    .map(|(_, id)| id.to_string())
                    .collect::<Vec<_>>(),
            );
            out.push_str("\n        ],\n");
            out.push_str("    },\n");
        }
    }
    out.push_str("];\n");
    out
}

// ---------------------------------------------------------------------------------------------
// entity properties
// ---------------------------------------------------------------------------------------------

fn gen_entity_props(path: &Path) -> String {
    let mut out = header("ferrite/entity_types.json");
    if !path.exists() {
        println!(
            "cargo:warning=vanilla entity data is missing at {}; run `python tools/fetch_vanilla.py` to generate it",
            path.display()
        );
        out.push_str(
            "/// Number of entity types; zero until `python tools/fetch_vanilla.py` has been rerun.\n\
             pub const ENTITY_TYPE_COUNT: usize = 0;\n\n",
        );
        out.push_str("static ENTITY_TYPES: &[EntityProps] = &[];\n\n");
        out.push_str("/// `(key, protocol id)` sorted by key for binary search.\n");
        out.push_str("static ENTITY_BY_NAME: &[(&str, u16)] = &[];\n");
        return out;
    }

    let report = read_json(path);
    let types = report
        .get("types")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{}: expected a types array", path.display()));
    let mut by_name: Vec<(&str, u16)> = Vec::with_capacity(types.len());

    writeln!(
        out,
        "/// Number of entity types.\npub const ENTITY_TYPE_COUNT: usize = {};\n",
        types.len()
    )
    .expect("writing to a String cannot fail");
    out.push_str("/// Entity type properties, indexed by protocol id.\nstatic ENTITY_TYPES: &[EntityProps] = &[\n");

    for (index, value) in types.iter().enumerate() {
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("{}: entity type {index} without an id", path.display()));
        if id != index as u64 {
            panic!(
                "{}: entity type ids are not contiguous; expected {index}, found {id}",
                path.display()
            );
        }
        let id = u16::try_from(id)
            .unwrap_or_else(|_| panic!("{}: too many entity types", path.display()));
        let key = value
            .get("key")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{}: entity type {id} without a key", path.display()));
        by_name.push((key, id));

        let category = value
            .get("category")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{key}: entity type without a category"));

        out.push_str("    EntityProps {\n");
        writeln!(out, "        id: {id},").expect("writing to a String cannot fail");
        writeln!(out, "        key: {},", literal(key)).expect("writing to a String cannot fail");
        writeln!(out, "        category: {},", literal(category))
            .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        width: {},",
            float_literal(entity_number(value, "width", key))
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        height: {},",
            float_literal(entity_number(value, "height", key))
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        eye_height: {},",
            float_literal(entity_number(value, "eyeHeight", key))
        )
        .expect("writing to a String cannot fail");
        writeln!(out, "        fixed: {},", entity_bool(value, "fixed", key))
            .expect("writing to a String cannot fail");

        let no_attachments = serde_json::Map::new();
        let attachment_json = value
            .get("attachments")
            .and_then(serde_json::Value::as_object)
            .unwrap_or(&no_attachments);
        let mut attachments: BTreeMap<&str, [f32; 3]> = BTreeMap::new();
        for (name, point) in attachment_json {
            let coordinates = point
                .as_array()
                .unwrap_or_else(|| panic!("{key}: attachment {name} is not an array"));
            if coordinates.len() != 3 {
                panic!(
                    "{key}: attachment {name} has {} coordinates; expected 3",
                    coordinates.len()
                );
            }
            let mut xyz = [0.0f32; 3];
            for (slot, coordinate) in coordinates.iter().enumerate() {
                xyz[slot] = coordinate.as_f64().unwrap_or_else(|| {
                    panic!("{key}: attachment {name} has a non-numeric coordinate")
                }) as f32;
            }
            attachments.insert(name, xyz);
        }
        if attachments.is_empty() {
            out.push_str("        attachments: &[],\n");
        } else {
            out.push_str("        attachments: &[\n");
            let rendered: Vec<String> = attachments
                .iter()
                .map(|(name, [x, y, z])| {
                    format!(
                        "({}, [{}, {}, {}])",
                        literal(name),
                        float_literal(f64::from(*x)),
                        float_literal(f64::from(*y)),
                        float_literal(f64::from(*z))
                    )
                })
                .collect();
            items(&mut out, "            ", &rendered);
            out.push_str("\n        ],\n");
        }

        writeln!(
            out,
            "        client_tracking_range: {},",
            entity_integer(value, "clientTrackingRange", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        update_interval: {},",
            entity_integer(value, "updateInterval", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        fire_immune: {},",
            entity_bool(value, "fireImmune", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        can_spawn_far_from_player: {},",
            entity_bool(value, "canSpawnFarFromPlayer", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        is_summonable: {},",
            entity_bool(value, "isSummonable", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        is_serializable: {},",
            entity_bool(value, "isSerializable", key)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "        can_be_serialized: {},",
            entity_bool(value, "canBeSerialized", key)
        )
        .expect("writing to a String cannot fail");

        let no_attributes = serde_json::Map::new();
        let attribute_json = value
            .get("attributes")
            .and_then(serde_json::Value::as_object)
            .unwrap_or(&no_attributes);
        let mut attributes: BTreeMap<&str, f64> = BTreeMap::new();
        for (name, attribute) in attribute_json {
            let attribute = attribute
                .as_f64()
                .unwrap_or_else(|| panic!("{key}: attribute {name} is not a number"));
            attributes.insert(name, attribute);
        }
        if attributes.is_empty() {
            out.push_str("        attributes: &[],\n");
        } else {
            out.push_str("        attributes: &[\n");
            let rendered: Vec<String> = attributes
                .iter()
                .map(|(name, attribute)| format!("({}, {attribute:?})", literal(name)))
                .collect();
            items(&mut out, "            ", &rendered);
            out.push_str("\n        ],\n");
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");

    by_name.sort_unstable_by(|left, right| left.0.cmp(right.0));
    out.push_str("/// `(key, protocol id)` sorted by key for binary search.\n");
    out.push_str("static ENTITY_BY_NAME: &[(&str, u16)] = &[\n");
    for (key, id) in by_name {
        writeln!(out, "    ({}, {id}),", literal(key)).expect("writing to a String cannot fail");
    }
    out.push_str("];\n");

    out
}

fn entity_number(value: &serde_json::Value, field: &str, key: &str) -> f64 {
    value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_else(|| panic!("{key}: entity type without a numeric {field}"))
}

fn entity_integer(value: &serde_json::Value, field: &str, key: &str) -> i32 {
    let number = value
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_else(|| panic!("{key}: entity type without an integer {field}"));
    i32::try_from(number)
        .unwrap_or_else(|_| panic!("{key}: entity type {field} {number} does not fit in i32"))
}

fn entity_bool(value: &serde_json::Value, field: &str, key: &str) -> bool {
    value
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| panic!("{key}: entity type without a boolean {field}"))
}

// ---------------------------------------------------------------------------------------------
// ---------------------------------------------------------------------------------------------
// block properties
// ---------------------------------------------------------------------------------------------

/// The empty shape, always index 0 of the generated shape table.
const EMPTY_SHAPE: &str = "&[]";
/// The full cube shape, always index 1 of the generated shape table.
const FULL_SHAPE: &str = "&[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]";

fn gen_block_props(path: &Path) -> String {
    let report = read_json(path);
    let states = report
        .get("states")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{}: expected a states array", path.display()));

    let mut props: Vec<String> = Vec::new();
    let mut prop_ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut state_props: Vec<usize> = Vec::with_capacity(states.len());
    let mut shapes: Vec<String> = vec![EMPTY_SHAPE.to_string(), FULL_SHAPE.to_string()];
    let mut shape_ids: BTreeMap<String, usize> = BTreeMap::new();
    shape_ids.insert(EMPTY_SHAPE.to_string(), 0);
    shape_ids.insert(FULL_SHAPE.to_string(), 1);
    let mut state_collision: Vec<usize> = Vec::with_capacity(states.len());
    let mut state_occlusion: Vec<usize> = Vec::with_capacity(states.len());
    let mut state_faces: Vec<[usize; 6]> = Vec::with_capacity(states.len());

    for (index, state) in states.iter().enumerate() {
        let id = u16::try_from(index)
            .unwrap_or_else(|_| panic!("{}: too many block states", path.display()));
        let reported = integer(state, "id", id);
        if reported != u64::from(id) {
            panic!(
                "{}: expected state {id}, found id {reported}",
                path.display()
            );
        }

        let fluid = match state.get("fluid") {
            Some(serde_json::Value::Null) | None => "None".to_string(),
            Some(fluid) => {
                let kind = match text(fluid, "id", id) {
                    "minecraft:water" => "FluidKind::Water".to_string(),
                    "minecraft:flowing_water" => "FluidKind::FlowingWater".to_string(),
                    "minecraft:lava" => "FluidKind::Lava".to_string(),
                    "minecraft:flowing_lava" => "FluidKind::FlowingLava".to_string(),
                    other => format!("FluidKind::Other({})", literal(other)),
                };
                format!(
                    "Some(({kind}, {}, {}))",
                    integer(fluid, "amount", id),
                    boolean(fluid, "falling", id)
                )
            }
        };
        let entry = format!(
            "BlockProps {{ light_emission: {}, light_opacity: {}, can_occlude: {}, \
             use_shape_for_light_occlusion: {}, propagates_skylight_down: {}, is_air: {}, \
             blocks_motion: {}, is_solid_render: {}, is_redstone_conductor: {}, fluid: {fluid}, \
             hardness: {}, requires_correct_tool: {}, replaceable: {}, full_collision: {}, \
             empty_collision: {}, full_occlusion: {}, empty_occlusion: {}, heightmaps: {}, randomly_ticking: {}, is_signal_source: {}, \
             has_analog_output: {}, friction: {}, speed_factor: {}, jump_factor: {} }},",
            integer(state, "lightEmission", id),
            integer(state, "lightOpacity", id),
            boolean(state, "canOcclude", id),
            boolean(state, "useShapeForLightOcclusion", id),
            boolean(state, "propagatesSkylightDown", id),
            boolean(state, "isAir", id),
            boolean(state, "blocksMotion", id),
            boolean(state, "isSolidRender", id),
            boolean(state, "isRedstoneConductor", id),
            float(state, "hardness", id),
            boolean(state, "requiresCorrectTool", id),
            boolean(state, "replaceable", id),
            shape_is(state, "collision", "full", id),
            shape_is(state, "collision", "empty", id),
            shape_is(state, "occlusion", "full", id),
            shape_is(state, "occlusion", "empty", id),
            integer(state, "heightmaps", id),
            boolean(state, "randomlyTicking", id),
            boolean(state, "isSignalSource", id),
            boolean(state, "hasAnalogOutput", id),
            float(state, "friction", id),
            float(state, "speedFactor", id),
            float(state, "jumpFactor", id),
        );
        let prop = *prop_ids.entry(entry.clone()).or_insert_with(|| {
            props.push(entry);
            props.len() - 1
        });
        state_props.push(prop);
        state_collision.push(shape_index(
            &mut shapes,
            &mut shape_ids,
            state,
            "collision",
            id,
        ));
        let faces = state["faces"].as_array().expect("faces array");
        assert_eq!(faces.len(), 6);
        let mut face_ids = [0; 6];
        for (i, face) in faces.iter().enumerate() {
            let wrapper = serde_json::json!({"face": face});
            face_ids[i] = shape_index(&mut shapes, &mut shape_ids, &wrapper, "face", id);
        }
        state_faces.push(face_ids);
        state_occlusion.push(shape_index(
            &mut shapes,
            &mut shape_ids,
            state,
            "occlusion",
            id,
        ));
    }

    let mut out = header("ferrite/block_states.json");
    writeln!(
        out,
        "/// Number of block states covered by the generated tables.\n\
         pub const STATE_COUNT: usize = {};\n",
        states.len()
    )
    .expect("writing to a String cannot fail");

    out.push_str("/// Deduplicated block properties, referenced by [`STATE_PROPS`].\n");
    out.push_str("static PROPS: &[BlockProps] = &[\n");
    for prop in &props {
        out.push_str("    ");
        out.push_str(prop);
        out.push('\n');
    }
    out.push_str("];\n\n");

    out.push_str("/// State id -> index into [`PROPS`].\nstatic STATE_PROPS: &[u16] = &[\n");
    let rows: Vec<String> = state_props.iter().map(usize::to_string).collect();
    items(&mut out, "    ", &rows);
    out.push_str("\n];\n\n");

    out.push_str(
        "/// Deduplicated shapes, each box as `[min_x, min_y, min_z, max_x, max_y, max_z]`.\n",
    );
    out.push_str("static SHAPES: &[&[[f64; 6]]] = &[\n");
    for shape in &shapes {
        out.push_str("    ");
        out.push_str(shape);
        out.push_str(",\n");
    }
    out.push_str("];\n\n");

    out.push_str("/// State id -> index into [`SHAPES`] for the collision shape.\n");
    out.push_str("static STATE_COLLISION: &[u16] = &[\n");
    let rows: Vec<String> = state_collision.iter().map(usize::to_string).collect();
    items(&mut out, "    ", &rows);
    out.push_str("\n];\n\n");

    out.push_str("/// State id -> index into [`SHAPES`] for the occlusion shape.\n");
    out.push_str("static STATE_OCCLUSION: &[u16] = &[\n");
    let rows: Vec<String> = state_occlusion.iter().map(usize::to_string).collect();
    items(&mut out, "    ", &rows);
    out.push_str("\n];\n\n");

    out.push_str("static STATE_FACES: &[[u16; 6]] = &[\n");
    for faces in &state_faces {
        writeln!(out, "    {:?},", faces).unwrap();
    }
    out.push_str("];\n");
    out.push_str("/// Per-block metadata, sorted by `name` for binary search.\n");
    out.push_str("static BLOCK_INFO: &[BlockInfo] = &[\n");
    let blocks = report
        .get("blocks")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("{}: expected a blocks array", path.display()));
    let mut blocks: Vec<&serde_json::Value> = blocks.iter().collect();
    blocks.sort_unstable_by_key(|block| block.get("name").and_then(serde_json::Value::as_str));
    for block in blocks {
        let name = block
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("{}: block without a name", path.display()));
        let default_state = block
            .get("defaultState")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("{name}: block without a default state"));
        let resistance = block
            .get("explosionResistance")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_else(|| panic!("{name}: block without an explosion resistance"));
        let item = match block.get("item") {
            Some(serde_json::Value::String(item)) => format!("Some({})", literal(item)),
            _ => "None".to_string(),
        };
        writeln!(
            out,
            "    BlockInfo {{ name: {}, default_state: {default_state}, \
             explosion_resistance: {}, item: {item} }},",
            literal(name),
            float_literal(resistance)
        )
        .expect("writing to a String cannot fail");
    }
    out.push_str("];\n");

    out
}

/// A string field of a JSON object, panicking with the state id when it is missing.
fn text<'a>(value: &'a serde_json::Value, field: &str, id: u16) -> &'a str {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("block state {id} has no string field {field}"))
}

/// A boolean field of a JSON object, panicking with the state id when it is missing.
fn boolean(value: &serde_json::Value, field: &str, id: u16) -> bool {
    value
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| panic!("block state {id} has no boolean field {field}"))
}

/// An integer field of a JSON object, panicking with the state id when it is missing.
fn integer(value: &serde_json::Value, field: &str, id: u16) -> u64 {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("block state {id} has no integer field {field}"))
}

/// Whether a state's `"empty"`/`"full"`/box shape is the expected kind.
fn shape_is(state: &serde_json::Value, field: &str, expected: &str, id: u16) -> bool {
    match state.get(field) {
        Some(serde_json::Value::String(kind)) => kind == expected,
        Some(serde_json::Value::Object(_)) => expected == "custom",
        other => panic!("block state {id} has an unexpected {field} shape: {other:?}"),
    }
}

/// The index of a state's shape in the deduplicated shape table.
fn shape_index(
    shapes: &mut Vec<String>,
    shape_ids: &mut BTreeMap<String, usize>,
    state: &serde_json::Value,
    field: &str,
    id: u16,
) -> usize {
    let shape = state
        .get(field)
        .unwrap_or_else(|| panic!("block state {id} has no {field} shape"));
    let entry = match shape {
        serde_json::Value::String(kind) if kind == "empty" => EMPTY_SHAPE.to_string(),
        serde_json::Value::String(kind) if kind == "full" => FULL_SHAPE.to_string(),
        serde_json::Value::Object(object) => {
            let boxes = object
                .get("boxes")
                .and_then(serde_json::Value::as_array)
                .unwrap_or_else(|| panic!("block state {id} has a {field} shape without boxes"));
            let mut entry = String::from("&[");
            for (index, box_value) in boxes.iter().enumerate() {
                let coordinates = box_value.as_array().unwrap_or_else(|| {
                    panic!("block state {id} has a {field} box that is not an array")
                });
                if coordinates.len() != 6 {
                    panic!(
                        "block state {id} has a {field} box with {} coordinates; expected 6",
                        coordinates.len()
                    );
                }
                if index > 0 {
                    entry.push_str(", ");
                }
                entry.push('[');
                for (corner, coordinate) in coordinates.iter().enumerate() {
                    if corner > 0 {
                        entry.push_str(", ");
                    }
                    let value = coordinate.as_f64().unwrap_or_else(|| {
                        panic!("block state {id} has a non-numeric {field} coordinate")
                    });
                    entry.push_str(&format!("{value:?}"));
                }
                entry.push(']');
            }
            entry.push(']');
            entry
        }
        other => panic!("block state {id} has an unexpected {field} shape: {other:?}"),
    };
    *shape_ids.entry(entry.clone()).or_insert_with(|| {
        shapes.push(entry);
        shapes.len() - 1
    })
}

/// A numeric field of a JSON object, rendered as a Rust `f32` literal.
fn float(value: &serde_json::Value, field: &str, id: u16) -> String {
    let value = value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_else(|| panic!("block state {id} has no numeric field {field}"));
    float_literal(value)
}

/// Renders a number as a Rust `f32` literal, handling the non-finite values vanilla uses.
fn float_literal(value: f64) -> String {
    let value = value as f32;
    if value.is_nan() {
        "f32::NAN".to_string()
    } else if value == f32::INFINITY {
        "f32::INFINITY".to_string()
    } else if value == f32::NEG_INFINITY {
        "f32::NEG_INFINITY".to_string()
    } else {
        format!("{value:?}")
    }
}

// ---------------------------------------------------------------------------------------------
// villager trades
// ---------------------------------------------------------------------------------------------

/// Item protocol ids from the `minecraft:item` registry report.
fn item_ids(registries: &Path) -> BTreeMap<String, u16> {
    let report = read_json(registries);
    report
        .get("minecraft:item")
        .and_then(|registry| registry.get("entries"))
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("{}: no minecraft:item registry", registries.display()))
        .iter()
        .map(|(name, value)| {
            let id = value
                .get("protocol_id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_else(|| panic!("item {name} has no protocol id"));
            (
                name.clone(),
                u16::try_from(id).unwrap_or_else(|_| panic!("item {name} id {id} is not a u16")),
            )
        })
        .collect()
}

/// An item and a count from a trade's `wants`/`gives` object.
fn trade_cost(
    value: &serde_json::Value,
    items: &BTreeMap<String, u16>,
    context: &str,
) -> (u16, i32) {
    let name = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{context}: cost without an id"));
    let count = value
        .get("count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1)
        .max(1) as i32;
    let item = *items
        .get(name)
        .unwrap_or_else(|| panic!("{context}: unknown item {name}"));
    (item, count)
}

/// The `*.json` files of a directory, sorted by path.
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

/// One generated trade definition.
struct TradeOut {
    name: String,
    wants: u16,
    wants_count: i32,
    gives: u16,
    gives_count: i32,
    second_wants: Option<(u16, i32)>,
    max_uses: i32,
    xp: i32,
    price_multiplier: f32,
}

/// One generated trade-set entry: a profession level's pool and how many offers it draws.
struct SetOut {
    profession: String,
    level: u8,
    amount: usize,
    trades: Vec<String>,
}

/// Compiles `villager_trade/**`, `trade_set/**` and the `villager_trade` tags into static tables.
///
/// Offers that need a `merchant_predicate` (a villager variant the offer depends on) are
/// skipped; trades that attach modifiers or enchantments to their result keep their base item,
/// because the item component model cannot apply those modifiers yet.
fn gen_villager_trades(
    registries: &Path,
    trades_root: &Path,
    sets_root: &Path,
    tags_root: &Path,
) -> String {
    let item_table = item_ids(registries);
    let mut defs = Vec::new();
    let mut professions: Vec<String> = fs::read_dir(trades_root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", trades_root.display()))
        .filter_map(|entry| {
            let path = entry.expect("directory entry").path();
            path.is_dir()
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .filter(|name| name != "wandering_trader")
        .collect();
    professions.sort();
    for profession in &professions {
        for level_dir in json_files_dirs(&trades_root.join(profession)) {
            let Some(level) = level_dir
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<u8>().ok())
            else {
                continue;
            };
            for file in json_files(&level_dir) {
                let stem = file
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_else(|| panic!("{}: no file name", file.display()));
                let json = read_json(&file);
                let context = format!("{profession}/{level}/{stem}");
                if json.get("merchant_predicate").is_some() {
                    continue;
                }
                let (wants, wants_count) = trade_cost(&json["wants"], &item_table, &context);
                let (gives, gives_count) = trade_cost(&json["gives"], &item_table, &context);
                let second_wants = json
                    .get("additional_wants")
                    .map(|wanted| trade_cost(wanted, &item_table, &context));
                let max_uses = json
                    .get("max_uses")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(12) as i32;
                let xp = json
                    .get("xp")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0) as i32;
                let price_multiplier = json
                    .get("reputation_discount")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.05) as f32;
                defs.push(TradeOut {
                    name: format!("minecraft:{profession}/{level}/{stem}"),
                    wants,
                    wants_count,
                    gives,
                    gives_count,
                    second_wants,
                    max_uses,
                    xp,
                    price_multiplier,
                });
            }
        }
    }
    defs.sort_by(|left, right| left.name.cmp(&right.name));

    let mut sets = Vec::new();
    for profession in &professions {
        let tag_dir = tags_root.join(profession);
        if !tag_dir.is_dir() {
            continue;
        }
        for file in json_files(&tag_dir) {
            let Some(level) = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.strip_prefix("level_"))
                .and_then(|level| level.parse::<u8>().ok())
            else {
                continue;
            };
            let Some(values) = read_json(&file)
                .get("values")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            else {
                continue;
            };
            let amount = read_json(
                &sets_root
                    .join(profession)
                    .join(format!("level_{level}.json")),
            )
            .get("amount")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(2) as usize;
            let mut trades: Vec<String> = values
                .into_iter()
                .filter(|name| {
                    defs.binary_search_by(|def| def.name.as_str().cmp(name))
                        .is_ok()
                })
                .collect();
            trades.sort();
            trades.dedup();
            if trades.is_empty() {
                continue;
            }
            sets.push(SetOut {
                profession: format!("minecraft:{profession}"),
                level,
                amount,
                trades,
            });
        }
    }
    sets.sort_by(|left, right| {
        (left.profession.as_str(), left.level).cmp(&(right.profession.as_str(), right.level))
    });

    let mut out = header("data/minecraft/villager_trade + trade_set + tags/villager_trade");
    out.push_str("/// The trade definitions, sorted by name for binary search.\n");
    out.push_str("static TRADES: &[TradeDef] = &[\n");
    for def in &defs {
        let second = match def.second_wants {
            Some((item, count)) => format!("Some(({item}, {count}))"),
            None => "None".into(),
        };
        writeln!(
            out,
            "    TradeDef {{ name: {}, wants: {}, wants_count: {}, gives: {}, gives_count: {}, \
             second_wants: {}, max_uses: {}, xp: {}, price_multiplier: {} }},",
            literal(&def.name),
            def.wants,
            def.wants_count,
            def.gives,
            def.gives_count,
            second,
            def.max_uses,
            def.xp,
            def.price_multiplier
        )
        .expect("writing to a String cannot fail");
    }
    out.push_str("];\n\n");

    out.push_str("/// The profession level pools, sorted by profession and level.\n");
    out.push_str("static TRADE_SETS: &[TradeSet] = &[\n");
    for set in &sets {
        writeln!(
            out,
            "    TradeSet {{ profession: {}, level: {}, amount: {}, trades: &[",
            literal(&set.profession),
            set.level,
            set.amount
        )
        .expect("writing to a String cannot fail");
        items(
            &mut out,
            "        ",
            &strings(&set.trades.iter().map(String::as_str).collect::<Vec<_>>()),
        );
        out.push_str("\n    ] },\n");
    }
    out.push_str("];\n");
    out
}

/// Subdirectories of a directory sorted by name.
fn json_files_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let path = entry.expect("directory entry").path();
                    path.is_dir().then_some(path)
                })
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}
