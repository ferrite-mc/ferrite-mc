//! Loot table loading, reference resolution and the public registry.

use crate::condition::{self, Condition};
use crate::data::GameData;
use crate::entry::{Entry, EntryKind, LootPool};
use crate::files::{json_files, read_json, relative_stem};
use crate::function::{self, Function, FunctionKind};
use crate::number::NumberProvider;
use crate::tags::ItemTags;
use crate::{Definitions, Error, LootContext, LootStack, Result, Rng, normalize_id};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One loaded loot table.
///
/// References to other tables are inlined while loading, so a table can be
/// rolled without the rest of the registry.
#[derive(Debug, Clone)]
pub struct LootTable {
    id: String,
    kind: String,
    random_sequence: Option<String>,
    pools: Vec<LootPool>,
    functions: Vec<Function>,
}

impl LootTable {
    /// An empty table, used for references to tables this data pack lacks.
    pub(crate) fn empty(id: String) -> Self {
        Self {
            id,
            kind: String::new(),
            random_sequence: None,
            pools: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Namespaced table id (`minecraft:blocks/stone`).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The table's `type` (`minecraft:block`, `minecraft:chest`, ...).
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The vanilla `random_sequence` key, when present.
    pub fn random_sequence(&self) -> Option<&str> {
        self.random_sequence.as_deref()
    }

    /// The table's pools.
    pub fn pools(&self) -> &[LootPool] {
        &self.pools
    }

    /// Table-level functions applied to every produced stack.
    pub fn functions(&self) -> &[Function] {
        &self.functions
    }

    /// Rolls the table and returns its drops in generation order.
    ///
    /// Stacks are `(namespaced item id, count)`. Entries whose count ends at
    /// zero after functions are omitted; stacks are not merged.
    pub fn roll(&self, context: &LootContext, rng: &mut impl Rng) -> Vec<(String, i32)> {
        self.roll_stacks(context, rng)
            .into_iter()
            .map(|stack| (stack.item, stack.count))
            .collect()
    }

    /// Rolls the table like [`LootTable::roll`], keeping the components `copy_components`
    /// attached (see [`LootContext::block_entity_components`]).
    pub fn roll_stacks(&self, context: &LootContext, rng: &mut impl Rng) -> Vec<LootStack> {
        let mut drops = Vec::new();
        self.roll_into(context, rng, &mut drops);
        drops
    }

    pub(crate) fn roll_into(
        &self,
        context: &LootContext,
        rng: &mut impl Rng,
        out: &mut Vec<LootStack>,
    ) {
        let start = out.len();
        for pool in &self.pools {
            pool.roll_into(context, rng, out);
        }
        for stack in &mut out[start..] {
            function::apply(&self.functions, stack, context, rng);
        }
    }
}

/// Every loot table (and the tags/predicates they use) from one generated data
/// pack namespace.
#[derive(Debug)]
pub struct LootTables {
    definitions: std::sync::Arc<Definitions>,
    namespace: String,
    tables: BTreeMap<String, LootTable>,
    tag_count: usize,
    predicate_count: usize,
}

impl LootTables {
    /// Loads every table below a generated loot directory.
    ///
    /// `path` may be the `loot_table` directory itself, the namespace directory
    /// that contains it (`.../data/minecraft`), or the generated root
    /// (`.../generated`, where `data/minecraft` is used). Sibling `predicate`
    /// and `tags/item` directories are loaded when present.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let (base, loot_dir) = locate(path);
        let namespace = base
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("minecraft")
            .to_owned();

        let definitions = Definitions {
            namespace: namespace.clone(),
            predicates: load_predicates(&base.join("predicate"), &namespace)?,
            tags: ItemTags::load(&base.join("tags/item"), &namespace)?,
            data: std::sync::Arc::new(GameData::load(
                &base,
                &namespace,
                ItemTags::load(&base.join("tags/item"), &namespace)?,
            )?),
        };

        let mut raw = BTreeMap::new();
        for path in json_files(&loot_dir)? {
            let Some(relative) = relative_stem(&path, &loot_dir) else {
                continue;
            };
            raw.insert(format!("{namespace}:{relative}"), read_json(&path)?);
        }

        let tag_count = definitions.tags.len();
        let predicate_count = definitions.predicates.len();
        let definitions = std::sync::Arc::new(definitions);
        let mut loader = Loader {
            raw: &raw,
            definitions: &definitions,
            cache: BTreeMap::new(),
            stack: Vec::new(),
        };
        let mut tables = BTreeMap::new();
        for id in raw.keys() {
            tables.insert(id.clone(), loader.build(id)?);
        }

        Ok(Self {
            definitions,
            namespace,
            tables,
            tag_count,
            predicate_count,
        })
    }

    /// Parses one loot function (as written in a loot table or an item modifier) against this
    /// pack's registry data. `Err` names a function type this crate does not evaluate.
    pub fn parse_function(&self, value: &Value) -> std::result::Result<Function, String> {
        let function = function::parse(value, &self.definitions)
            .ok_or_else(|| "a loot function is a JSON object".to_owned())?;
        match &function.kind {
            FunctionKind::Opaque { kind, .. } => Err(format!("unsupported function '{kind}'")),
            _ => Ok(function),
        }
    }

    /// Looks up a table by id. An omitted namespace defaults to the loaded
    /// pack's namespace.
    pub fn get(&self, id: &str) -> Option<&LootTable> {
        self.tables.get(&qualify(id, &self.namespace))
    }

    /// Number of loaded tables.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Whether no tables were loaded.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Every loaded table id, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.tables.keys().map(String::as_str)
    }

    /// Every function, condition and entry type the shipped data uses that this crate does not
    /// evaluate (`function:minecraft:...`), sorted. Empty means every loaded table rolls fully.
    pub fn unsupported(&self) -> Vec<String> {
        let mut found = std::collections::BTreeSet::new();
        for table in self.tables.values() {
            walk_table(table, &mut found);
        }
        found.into_iter().collect()
    }

    /// Number of resolved item tags.
    pub fn tag_count(&self) -> usize {
        self.tag_count
    }

    /// Number of loaded predicate files.
    pub fn predicate_count(&self) -> usize {
        self.predicate_count
    }
}

/// Resolves the namespace base directory and the loot table directory.
fn locate(path: &Path) -> (PathBuf, PathBuf) {
    let nested = path.join("loot_table");
    if nested.is_dir() {
        return (path.to_path_buf(), nested);
    }
    let generated = path.join("data/minecraft/loot_table");
    if generated.is_dir() {
        let base = path.join("data/minecraft");
        return (base.clone(), generated);
    }
    let base = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    (base, path.to_path_buf())
}

fn load_predicates(dir: &Path, namespace: &str) -> Result<BTreeMap<String, Value>> {
    let mut predicates = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(predicates);
    }
    for path in json_files(dir)? {
        let Some(relative) = relative_stem(&path, dir) else {
            continue;
        };
        predicates.insert(format!("{namespace}:{relative}"), read_json(&path)?);
    }
    Ok(predicates)
}

fn qualify(id: &str, namespace: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("{namespace}:{id}")
    }
}

/// Builds tables on demand, inlining references and rejecting cycles.
struct Loader<'a> {
    raw: &'a BTreeMap<String, Value>,
    definitions: &'a Definitions,
    cache: BTreeMap<String, LootTable>,
    stack: Vec<String>,
}

impl Loader<'_> {
    fn build(&mut self, id: &str) -> Result<LootTable> {
        if let Some(table) = self.cache.get(id) {
            return Ok(table.clone());
        }
        if self.stack.iter().any(|current| current == id) {
            return Err(Error::Cycle(id.to_owned()));
        }
        let Some(value) = self.raw.get(id).cloned() else {
            return Ok(LootTable::empty(id.to_owned()));
        };

        self.stack.push(id.to_owned());
        let table = self.parse_table(id, &value)?;
        self.stack.pop();
        self.cache.insert(id.to_owned(), table.clone());
        Ok(table)
    }

    fn parse_table(&mut self, id: &str, value: &Value) -> Result<LootTable> {
        let object = value.as_object();
        let kind = object
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let random_sequence = object
            .and_then(|object| object.get("random_sequence"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        let mut pools = Vec::new();
        if let Some(values) = object
            .and_then(|object| object.get("pools"))
            .and_then(Value::as_array)
        {
            for value in values {
                pools.push(self.parse_pool(value, id)?);
            }
        }

        let functions = object
            .map(|object| self.parse_functions(object))
            .unwrap_or_default();

        Ok(LootTable {
            id: id.to_owned(),
            kind,
            random_sequence,
            pools,
            functions,
        })
    }

    fn parse_pool(&mut self, value: &Value, parent_id: &str) -> Result<LootPool> {
        let object = value.as_object();
        let rolls = object.and_then(|object| object.get("rolls")).map_or_else(
            || NumberProvider::constant(1.0),
            |value| NumberProvider::parse(Some(value)),
        );
        let bonus_rolls = object
            .and_then(|object| object.get("bonus_rolls"))
            .map_or_else(
                || NumberProvider::constant(0.0),
                |value| NumberProvider::parse(Some(value)),
            );

        let mut entries = Vec::new();
        if let Some(values) = object
            .and_then(|object| object.get("entries"))
            .and_then(Value::as_array)
        {
            for value in values {
                entries.push(self.parse_entry(value, parent_id)?);
            }
        }

        Ok(LootPool {
            rolls,
            bonus_rolls,
            conditions: object
                .map(|object| self.parse_conditions(object))
                .unwrap_or_default(),
            functions: object
                .map(|object| self.parse_functions(object))
                .unwrap_or_default(),
            entries,
        })
    }

    fn parse_entry(&mut self, value: &Value, parent_id: &str) -> Result<Entry> {
        let Some(object) = value.as_object() else {
            return Ok(Entry {
                weight: 1,
                quality: 0,
                conditions: Vec::new(),
                functions: Vec::new(),
                kind: EntryKind::Empty,
            });
        };

        let weight = object
            .get("weight")
            .and_then(Value::as_i64)
            .map_or(1, |weight| i32::try_from(weight).unwrap_or(i32::MAX));
        let quality = object
            .get("quality")
            .and_then(Value::as_i64)
            .map_or(0, |quality| i32::try_from(quality).unwrap_or(i32::MAX));
        let kind_name = object.get("type").and_then(Value::as_str).unwrap_or("");
        let kind = match kind_name {
            "minecraft:item" => match object.get("name").and_then(Value::as_str) {
                Some(name) => EntryKind::Item {
                    name: normalize_id(name),
                },
                None => EntryKind::Empty,
            },
            "minecraft:tag" => {
                let mut items = Vec::new();
                condition::collect_item_refs(object.get("items"), self.definitions, &mut items);
                items.sort();
                items.dedup();
                EntryKind::Tag {
                    tag: object
                        .get("items")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    expand: object
                        .get("expand")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    items,
                }
            }
            "minecraft:alternatives" => {
                EntryKind::Alternatives(self.parse_children(object, parent_id)?)
            }
            "minecraft:sequence" => EntryKind::Sequence(self.parse_children(object, parent_id)?),
            "minecraft:group" => EntryKind::Group(self.parse_children(object, parent_id)?),
            "minecraft:loot_table" => self.parse_table_entry(object, parent_id)?,
            "minecraft:dynamic" => EntryKind::Dynamic {
                name: object
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            "minecraft:empty" => EntryKind::Empty,
            other => EntryKind::Opaque {
                kind: other.to_owned(),
            },
        };

        Ok(Entry {
            weight,
            quality,
            conditions: self.parse_conditions(object),
            functions: self.parse_functions(object),
            kind,
        })
    }

    fn parse_children(
        &mut self,
        object: &Map<String, Value>,
        parent_id: &str,
    ) -> Result<Vec<Entry>> {
        let mut children = Vec::new();
        for key in ["children", "entries"] {
            if let Some(values) = object.get(key).and_then(Value::as_array) {
                for value in values {
                    children.push(self.parse_entry(value, parent_id)?);
                }
            }
        }
        Ok(children)
    }

    fn parse_table_entry(
        &mut self,
        object: &Map<String, Value>,
        parent_id: &str,
    ) -> Result<EntryKind> {
        match object.get("value").or_else(|| object.get("name")) {
            Some(Value::String(id)) => {
                let id = qualify(id, &self.definitions.namespace);
                Ok(EntryKind::Table(Box::new(self.build(&id)?)))
            }
            Some(value @ Value::Object(_)) => {
                let value = value.clone();
                let id = format!("{parent_id}#inline");
                Ok(EntryKind::Table(Box::new(self.parse_table(&id, &value)?)))
            }
            _ => Ok(EntryKind::Empty),
        }
    }

    fn parse_conditions(&self, object: &Map<String, Value>) -> Vec<Condition> {
        let mut conditions = Vec::new();
        for key in ["condition", "conditions"] {
            if let Some(value) = object.get(key) {
                conditions.extend(condition::parse_list(value, self.definitions));
            }
        }
        conditions
    }

    fn parse_functions(&self, object: &Map<String, Value>) -> Vec<Function> {
        let mut functions = Vec::new();
        for key in ["modifier", "functions"] {
            if let Some(value) = object.get(key) {
                functions.extend(function::parse_functions(value, self.definitions));
            }
        }
        functions
    }
}

fn walk_table(table: &LootTable, found: &mut std::collections::BTreeSet<String>) {
    walk_functions(&table.functions, found);
    for pool in &table.pools {
        walk_conditions(&pool.conditions, found);
        walk_functions(&pool.functions, found);
        for entry in &pool.entries {
            walk_entry(entry, found);
        }
    }
}

fn walk_entry(entry: &Entry, found: &mut std::collections::BTreeSet<String>) {
    walk_conditions(&entry.conditions, found);
    walk_functions(&entry.functions, found);
    match &entry.kind {
        EntryKind::Opaque { kind } => {
            found.insert(format!("entry:{kind}"));
        }
        EntryKind::Alternatives(children)
        | EntryKind::Sequence(children)
        | EntryKind::Group(children) => {
            for child in children {
                walk_entry(child, found);
            }
        }
        EntryKind::Table(table) => walk_table(table, found),
        _ => {}
    }
}

fn walk_functions(functions: &[Function], found: &mut std::collections::BTreeSet<String>) {
    for function in functions {
        walk_conditions(&function.conditions, found);
        match &function.kind {
            FunctionKind::Opaque { kind, .. } => {
                found.insert(format!("function:{kind}"));
            }
            FunctionKind::Filtered {
                on_pass, on_fail, ..
            } => {
                walk_functions(on_pass, found);
                walk_functions(on_fail, found);
            }
            _ => {}
        }
    }
}

fn walk_conditions(conditions: &[Condition], found: &mut std::collections::BTreeSet<String>) {
    for condition in conditions {
        match condition {
            Condition::Opaque { kind } => {
                found.insert(format!("condition:{kind}"));
            }
            Condition::Inverted(inner) => walk_conditions(std::slice::from_ref(inner), found),
            Condition::AnyOf(inner) | Condition::AllOf(inner) => walk_conditions(inner, found),
            _ => {}
        }
    }
}
