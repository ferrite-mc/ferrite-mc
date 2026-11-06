//! Compiles the 26.3 registries and item component reports into static Rust tables.
//!
//! `reports/registries.json` supplies the `minecraft:data_component_type` and
//! `minecraft:item` protocol ids. The default components of an item come from the 26.3 data
//! generator's per-item report at `reports/minecraft/components/item/<name>.json`; there is no
//! `reports/items.json` in this version. Those component values are serialized as nameless-root
//! NBT into `defaults.nbt`, which the generated `items.rs` embeds with `include_bytes!`, so the
//! crate parses no JSON at runtime.
//!
//! The vanilla data is produced by `python tools/fetch_vanilla.py`; this script only reads it
//! and fails with a pointer to that command when it is missing.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ferrite_nbt::{Compound, List, Tag};

/// Minecraft version whose generated data is compiled in.
const VERSION: &str = "26.3";

fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for build scripts"),
    );
    // crates/ferrite-item -> crates -> <workspace>
    let workspace = manifest_dir
        .ancestors()
        .nth(2)
        .expect("ferrite-item lives in <workspace>/crates/ferrite-item")
        .to_path_buf();
    let generated = workspace.join("vanilla").join(VERSION).join("generated");
    let registries = generated.join("reports").join("registries.json");
    let item_reports = generated
        .join("reports")
        .join("minecraft")
        .join("components")
        .join("item");
    let enchantments = generated.join("data").join("minecraft").join("enchantment");
    for path in [&registries, &item_reports, &enchantments] {
        if !path.exists() {
            missing(path);
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    for path in [&registries, &item_reports, &enchantments] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let registry = read_json(&registries);
    let component_types = registry_entries(&registry, "minecraft:data_component_type");
    let items = registry_entries(&registry, "minecraft:item");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set for build scripts"));
    write_file(
        &out_dir.join("component_ids.rs"),
        &gen_component_ids(&component_types),
    );
    let (items_source, default_components) = gen_items(&items, &component_types, &item_reports);
    write_file(&out_dir.join("items.rs"), &items_source);
    fs::write(out_dir.join("defaults.nbt"), default_components)
        .expect("failed to write defaults.nbt");
    write_file(
        &out_dir.join("wire_registries.rs"),
        &gen_wire_registries(&registry, &enchantments),
    );
}

// ---------------------------------------------------------------------------------------------
// network registry ids
// ---------------------------------------------------------------------------------------------

/// Built-in registries and the data-driven enchantment registry are what the typed network
/// components need to resolve names to ids and back. Blocks and attributes come from
/// `registries.json`; enchantment ids are the sorted order of `data/minecraft/enchantment`.
fn gen_wire_registries(registry: &serde_json::Value, enchantment_dir: &Path) -> String {
    let attributes = registry_entries(registry, "minecraft:attribute");
    let blocks = registry_entries(registry, "minecraft:block");
    let potions = registry_entries(registry, "minecraft:potion");
    let effects = registry_entries(registry, "minecraft:mob_effect");
    let mut enchantments: Vec<String> = fs::read_dir(enchantment_dir)
        .unwrap_or_else(|error| panic!("failed to list {}: {error}", enchantment_dir.display()))
        .map(|entry| entry.expect("the directory is readable").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter_map(|name| {
            name.strip_suffix(".json")
                .map(|stem| format!("minecraft:{stem}"))
        })
        .collect();
    enchantments.sort_unstable();
    assert!(
        !enchantments.is_empty(),
        "{} has no enchantment files",
        enchantment_dir.display()
    );

    let mut out = header("reports/registries.json and data/minecraft/enchantment");
    for (name, entries) in [
        ("ATTRIBUTES", &attributes),
        ("BLOCKS", &blocks),
        ("POTIONS", &potions),
        ("MOB_EFFECTS", &effects),
    ] {
        writeln!(
            out,
            "/// `{name}` entries sorted by name: `(name, protocol id)`.\nstatic {name}: &[(&str, u16)] = &["
        )
        .expect("writing to a String cannot fail");
        for (entry, id) in entries.iter() {
            writeln!(out, "    ({}, {id}),", literal(entry))
                .expect("writing to a String cannot fail");
        }
        out.push_str("];\n\n");

        let mut by_id: Vec<(&str, u16)> = entries.to_vec();
        by_id.sort_unstable_by_key(|(_, id)| *id);
        let names = name.trim_end_matches('S');
        writeln!(
            out,
            "/// `{name}` names by protocol id.\nstatic {names}_NAMES: &[&str] = &["
        )
        .expect("writing to a String cannot fail");
        for (entry, _) in &by_id {
            writeln!(out, "    {},", literal(entry)).expect("writing to a String cannot fail");
        }
        out.push_str("];\n\n");
    }

    out.push_str(
        "/// `minecraft:enchantment` names in registry id order (sorted by name).\n\
         static ENCHANTMENTS: &[&str] = &[\n",
    );
    for enchantment in &enchantments {
        writeln!(out, "    {},", literal(enchantment)).expect("writing to a String cannot fail");
    }
    out.push_str("];\n");
    // Datapack registries are numbered in sorted-name order, like the enchantments above.
    let data = enchantment_dir
        .parent()
        .expect("the enchantment dir has a parent");
    for (table, dir, doc) in [
        ("TRIM_MATERIALS", "trim_material", "trim materials"),
        ("TRIM_PATTERNS", "trim_pattern", "trim patterns"),
        ("JUKEBOX_SONGS", "jukebox_song", "jukebox songs"),
    ] {
        let mut names: Vec<String> = fs::read_dir(data.join(dir))
            .unwrap_or_else(|error| panic!("failed to list {}: {error}", data.join(dir).display()))
            .filter_map(|entry| {
                let name = entry.expect("the directory is readable").file_name();
                let name = name.to_string_lossy().into_owned();
                name.strip_suffix(".json")
                    .map(|stem| format!("minecraft:{stem}"))
            })
            .collect();
        names.sort_unstable();
        assert!(!names.is_empty(), "no {dir} files");
        writeln!(
            out,
            "\n/// The {doc} in registry id order (sorted by name).\nstatic {table}: &[&str] = &["
        )
        .expect("writing to a String cannot fail");
        for name in &names {
            writeln!(out, "    {},", literal(name)).expect("writing to a String cannot fail");
        }
        out.push_str("];\n");
    }
    out
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
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn header(report: &str) -> String {
    format!(
        "// @generated by crates/ferrite-item/build.rs from vanilla/{VERSION}/generated/{report}.\n\
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

/// Entries of one built-in registry as `(name, protocol id)`, sorted by name.
///
/// The entries of a registry are contiguous from id 0, which the dense runtime tables rely on;
/// anything else is reported as a data error instead of producing a wrong table.
fn registry_entries<'a>(registry: &'a serde_json::Value, name: &str) -> Vec<(&'a str, u16)> {
    let entries = registry
        .get(name)
        .and_then(|entry| entry.get("entries"))
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("reports/registries.json: registry {name} is missing"));
    let mut entries: Vec<(&str, u16)> = entries
        .iter()
        .map(|(entry, value)| {
            let id = value
                .get("protocol_id")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_else(|| panic!("{name}/{entry}: entry has no protocol_id"));
            let id = u16::try_from(id)
                .unwrap_or_else(|_| panic!("{name}/{entry}: protocol id {id} does not fit in u16"));
            (entry.as_str(), id)
        })
        .collect();
    entries.sort_unstable_by(|left, right| left.0.cmp(right.0));

    let mut ids: Vec<u16> = entries.iter().map(|(_, id)| *id).collect();
    ids.sort_unstable();
    if ids != (0..entries.len() as u16).collect::<Vec<_>>() {
        panic!("{name}: protocol ids are not contiguous from 0");
    }
    entries
}

// ---------------------------------------------------------------------------------------------
// component type ids
// ---------------------------------------------------------------------------------------------

fn gen_component_ids(types: &[(&str, u16)]) -> String {
    let mut by_id: Vec<(&str, u16)> = types.to_vec();
    by_id.sort_unstable_by_key(|(_, id)| *id);

    let mut out = header("reports/registries.json");
    writeln!(
        out,
        "/// Number of entries of the `minecraft:data_component_type` registry.\n\
         pub const COUNT: usize = {};\n",
        by_id.len()
    )
    .expect("writing to a String cannot fail");

    let mut names: BTreeMap<String, &str> = BTreeMap::new();
    for (name, id) in &by_id {
        let constant = constant_name(name);
        if let Some(other) = names.insert(constant.clone(), name) {
            panic!("component types {other} and {name} both map to constant {constant}");
        }
        writeln!(out, "/// `{name}`\npub const {constant}: u16 = {id};")
            .expect("writing to a String cannot fail");
    }

    out.push_str("\n/// Component type names by protocol id.\npub static NAMES: &[&str] = &[\n");
    for (name, _) in &by_id {
        writeln!(out, "    {},", literal(name)).expect("writing to a String cannot fail");
    }
    out.push_str("];\n");

    out.push_str(
        "\n/// Component types sorted by name: `(name, protocol id)`.\n\
         pub static TYPES: &[(&str, u16)] = &[\n",
    );
    for (name, id) in types {
        writeln!(out, "    ({}, {id}),", literal(name)).expect("writing to a String cannot fail");
    }
    out.push_str("];\n");
    out
}

/// The Rust constant that names a component type: `minecraft:cushion/color` becomes
/// `CUSHION_COLOR`.
fn constant_name(name: &str) -> String {
    let key = name.strip_prefix("minecraft:").unwrap_or(name);
    let mut out = String::with_capacity(key.len());
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

// ---------------------------------------------------------------------------------------------
// item defaults
// ---------------------------------------------------------------------------------------------

/// One row of the generated item table.
struct ItemRow {
    name: String,
    id: u16,
    max_stack_size: i32,
    max_damage: Option<i32>,
    /// The `Rarity` variant, e.g. `"Common"`.
    rarity: String,
    offset: u32,
    len: u32,
}

fn gen_items(
    items: &[(&str, u16)],
    component_types: &[(&str, u16)],
    dir: &Path,
) -> (String, Vec<u8>) {
    let mut blob: Vec<u8> = Vec::new();
    let mut rows: Vec<ItemRow> = Vec::with_capacity(items.len());

    for (name, id) in items {
        let file = dir.join(format!(
            "{}.json",
            name.strip_prefix("minecraft:").unwrap_or(name)
        ));
        if !file.exists() {
            panic!(
                "{}: item component report is missing\n\
                 run `python tools/fetch_vanilla.py` in the workspace root to regenerate it",
                file.display()
            );
        }
        let report = read_json(&file);
        let components = report
            .get("components")
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("{}: expected a components object", file.display()));
        for key in components.keys() {
            if !component_types
                .iter()
                .any(|(component, _)| component == key)
            {
                panic!("{name}: components list {key}, which is not a registered type");
            }
        }

        let max_stack_size = int_component(components, "minecraft:max_stack_size").unwrap_or(64);
        let max_damage = int_component(components, "minecraft:max_damage");
        let rarity = rarity_variant(
            string_component(components, "minecraft:rarity")
                .unwrap_or_else(|| panic!("{name}: no minecraft:rarity component")),
        );

        let tag = Tag::Compound(json_compound(components));
        let bytes = ferrite_nbt::binary::write_network(&tag)
            .unwrap_or_else(|error| panic!("{name}: cannot encode default components: {error}"));
        let offset = u32::try_from(blob.len()).expect("default component blob exceeds 4 GiB");
        blob.extend_from_slice(&bytes);
        let len = u32::try_from(bytes.len()).expect("one item's defaults exceed 4 GiB");
        rows.push(ItemRow {
            name: (*name).to_owned(),
            id: *id,
            max_stack_size,
            max_damage,
            rarity,
            offset,
            len,
        });
    }

    // Item protocol id -> row index. Registry ids are dense, so this table is exact.
    let mut by_id = vec![u16::MAX; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        let slot = &mut by_id[usize::from(row.id)];
        if *slot != u16::MAX {
            panic!("item id {} is used twice", row.id);
        }
        *slot = u16::try_from(index).expect("more items than a u16 index can address");
    }

    let mut out = header("reports/minecraft/components/item");
    out.push_str("use crate::component::Rarity;\n\n");
    writeln!(
        out,
        "/// Number of entries of the `minecraft:item` registry.\n\
         pub const ITEM_COUNT: usize = {};\n",
        rows.len()
    )
    .expect("writing to a String cannot fail");
    out.push_str(
        "/// One item's default components.\n\
         #[derive(Debug, PartialEq)]\n\
         pub struct ItemDefaults {\n\
         \x20   /// Vanilla name, e.g. `\"minecraft:stone\"`.\n\
         \x20   pub name: &'static str,\n\
         \x20   /// Protocol id in the `minecraft:item` registry.\n\
         \x20   pub id: u16,\n\
         \x20   /// Default `minecraft:max_stack_size` (64 when the item does not set one).\n\
         \x20   pub max_stack_size: i32,\n\
         \x20   /// Default `minecraft:max_damage`, for items that can take damage.\n\
         \x20   pub max_damage: Option<i32>,\n\
         \x20   /// Default `minecraft:rarity`.\n\
         \x20   pub rarity: Rarity,\n\
         \x20   /// Offset of the item's default components in `DEFAULTS_NBT`.\n\
         \x20   offset: u32,\n\
         \x20   /// Length of the item's default components in `DEFAULTS_NBT`.\n\
         \x20   len: u32,\n\
         }\n\n",
    );
    out.push_str("/// Item defaults, sorted by name for binary search.\nstatic ITEMS: &[ItemDefaults] = &[\n");
    for row in &rows {
        let max_damage = match row.max_damage {
            Some(value) => format!("Some({value})"),
            None => "None".to_owned(),
        };
        writeln!(
            out,
            "    ItemDefaults {{ name: {}, id: {}, max_stack_size: {}, max_damage: {}, \
             rarity: Rarity::{}, offset: {}, len: {} }},",
            literal(&row.name),
            row.id,
            row.max_stack_size,
            max_damage,
            row.rarity,
            row.offset,
            row.len,
        )
        .expect("writing to a String cannot fail");
    }
    out.push_str("];\n\n");

    out.push_str("/// Item protocol id -> index into [`ITEMS`].\nstatic ITEM_BY_ID: &[u16] = &[\n");
    for index in &by_id {
        writeln!(out, "    {index},").expect("writing to a String cannot fail");
    }
    out.push_str("];\n\n");

    out.push_str(
        "/// Default component values as nameless-root NBT, one compound per item, sliced by\n\
         /// [`ItemDefaults`]' offset and length.\n\
         static DEFAULTS_NBT: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/defaults.nbt\"));\n",
    );

    (out, blob)
}

/// An integer component value of an item report, if present.
fn int_component(
    components: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<i32> {
    let value = components.get(key)?;
    let number = value
        .as_i64()
        .unwrap_or_else(|| panic!("{key} is not an integer: {value}"));
    Some(i32::try_from(number).unwrap_or_else(|_| panic!("{key}: {number} does not fit in i32")))
}

/// A string component value of an item report, if present.
fn string_component<'a>(
    components: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    components.get(key).map(|value| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("{key} is not a string: {value}"))
    })
}

/// The `Rarity` variant of a report rarity name.
fn rarity_variant(rarity: &str) -> String {
    match rarity.strip_prefix("minecraft:").unwrap_or(rarity) {
        "common" => "Common".to_owned(),
        "uncommon" => "Uncommon".to_owned(),
        "rare" => "Rare".to_owned(),
        "epic" => "Epic".to_owned(),
        other => panic!("unknown rarity {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// JSON -> NBT
// ---------------------------------------------------------------------------------------------

fn json_compound(map: &serde_json::Map<String, serde_json::Value>) -> Compound {
    map.iter()
        .map(|(key, value)| (key.clone(), json_tag(value)))
        .collect()
}

/// Converts a JSON value of the report into NBT.
///
/// JSON has no integer/float distinction that matches Minecraft's codecs, so integers become
/// `TAG_Int` and anything written with a decimal point or exponent becomes `TAG_Double`.
/// Arrays must be homogeneous (as NBT lists are); the report never contains mixed ones.
fn json_tag(value: &serde_json::Value) -> Tag {
    match value {
        serde_json::Value::Null => panic!("null is not valid component data"),
        serde_json::Value::Bool(flag) => Tag::Byte(i8::from(*flag)),
        serde_json::Value::Number(number) => match number.as_i64() {
            Some(integer) => Tag::Int(
                i32::try_from(integer)
                    .unwrap_or_else(|_| panic!("{integer} does not fit in an NBT int")),
            ),
            None => Tag::Double(number.as_f64().expect("JSON numbers are finite")),
        },
        serde_json::Value::String(text) => Tag::String(text.clone()),
        serde_json::Value::Array(values) => Tag::List(json_list(values)),
        serde_json::Value::Object(map) => Tag::Compound(json_compound(map)),
    }
}

/// The element kind of a homogeneous JSON array.
#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Number,
    String,
    Object,
    Array,
}

impl Kind {
    fn of(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Bool(_) => Self::Bool,
            serde_json::Value::Number(_) => Self::Number,
            serde_json::Value::String(_) => Self::String,
            serde_json::Value::Object(_) => Self::Object,
            serde_json::Value::Array(_) => Self::Array,
            serde_json::Value::Null => panic!("null array elements are not supported"),
        }
    }

    fn matches(self, value: &serde_json::Value) -> bool {
        matches!(
            (self, value),
            (Self::Bool, serde_json::Value::Bool(_))
                | (Self::Number, serde_json::Value::Number(_))
                | (Self::String, serde_json::Value::String(_))
                | (Self::Object, serde_json::Value::Object(_))
                | (Self::Array, serde_json::Value::Array(_))
        )
    }
}

fn json_list(values: &[serde_json::Value]) -> List {
    let Some(first) = values.first() else {
        return List::End;
    };
    let kind = Kind::of(first);
    if values.iter().any(|value| !kind.matches(value)) {
        panic!("arrays with mixed element kinds are not supported: {values:?}");
    }
    match kind {
        Kind::Bool => List::Byte(
            values
                .iter()
                .map(|value| i8::from(value.as_bool().expect("checked by Kind")))
                .collect(),
        ),
        Kind::Number if values.iter().all(|value| value.as_i64().is_some()) => List::Int(
            values
                .iter()
                .map(|value| {
                    let integer = value.as_i64().expect("checked by the all() above");
                    i32::try_from(integer)
                        .unwrap_or_else(|_| panic!("{integer} does not fit in an NBT int"))
                })
                .collect(),
        ),
        Kind::Number => List::Double(
            values
                .iter()
                .map(|value| value.as_f64().expect("JSON numbers are finite"))
                .collect(),
        ),
        Kind::String => List::String(
            values
                .iter()
                .map(|value| value.as_str().expect("checked by Kind").to_owned())
                .collect(),
        ),
        Kind::Object => List::Compound(
            values
                .iter()
                .map(|value| json_compound(value.as_object().expect("checked by Kind")))
                .collect(),
        ),
        Kind::Array => List::List(
            values
                .iter()
                .map(|value| json_list(value.as_array().expect("checked by Kind")))
                .collect(),
        ),
    }
}
