//! Unlocks recipes from play, like the `recipes/**` advancements do: a recipe comes into the
//! book once the player holds an item its advancement asks for (`inventory_changed`), already
//! has the recipe (`recipe_unlocked`), enters a matching block (`enter_block`) or ticks
//! (`tick`). Counts and components are not evaluated. The rules are read from the extracted
//! advancement data.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

/// Item names each player holds, refreshed by the region threads on `Command::ScanInventory`.
pub static HELD: Mutex<BTreeMap<Uuid, BTreeSet<String>>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone)]
pub enum Criterion {
    /// Every entry must match a held item; an entry is `(names, is_tag)` alternatives.
    Holds(Vec<Vec<String>>),
    Recipe(String),
    /// `enter_block` with a block predicate.
    InBlock(BlockMatch),
    /// `tick` with no conditions; it fires for every player.
    Ticks,
    /// `player_killed_entity` with nothing but a plain entity type to match.
    Kills(String),
    /// `consume_item` of one item id, or any item (`*`).
    Consumes(String),
    /// `changed_dimension` into one dimension (no `from`).
    EntersDimension(String),
    /// `placed_block` of exactly one block id.
    Places(String),
    /// `location` with nothing but one plain biome id to match.
    InBiome(String),
    /// `recipe_crafted` of one recipe id with no ingredient condition.
    Crafts(String),
    /// `bred_animals` of one child entity type, or any child (`*`).
    Breeds(String),
    /// `tame_animal` with no conditions.
    Tames,
    /// `item_used_on_block` with a block, optional state, tool, biome and smoke condition.
    UsedOnBlock(UsedOnBlock),
    Other,
}

/// The conditions of an `item_used_on_block` criterion this server can check.
#[derive(Debug, Clone, Default)]
pub struct UsedOnBlock {
    /// Block ids or `#tags`; empty matches any block.
    pub blocks: Vec<String>,
    /// Block state properties that must match.
    pub state: Vec<(String, String)>,
    /// Item ids or `#tags` the player must hold; empty matches any item.
    pub items: Vec<String>,
    pub biome: Option<String>,
    /// A beehive with a lit campfire below it.
    pub smokey: bool,
    /// The held item must be playable in a jukebox (a music disc).
    pub playable: bool,
}

/// What a player used on a block, for [`criteria_for_used`].
#[derive(Debug, Clone)]
pub struct UsedRecord {
    pub player: Uuid,
    /// The block after the use.
    pub block: &'static str,
    pub props: &'static [(&'static str, &'static str)],
    /// The item held before the use.
    pub item: Option<&'static str>,
    pub biome: Option<String>,
    pub smokey: bool,
}

/// Uses of items on blocks since the last drain.
pub static USED_ON_BLOCK: Mutex<Vec<UsedRecord>> = Mutex::new(Vec::new());

/// The block predicate of an `enter_block` criterion.
#[derive(Debug, Clone, Default)]
pub struct BlockMatch {
    /// Block ids or `#tags`; empty matches no block.
    pub blocks: Vec<String>,
    /// Block state properties that must match.
    pub state: Vec<(String, String)>,
}

impl BlockMatch {
    fn matches(&self, block: &str, props: &[(&str, &str)]) -> bool {
        !self.blocks.is_empty()
            && id_matches(&self.blocks, "block", block)
            && self.state.iter().all(|(key, value)| {
                props
                    .iter()
                    .any(|(k, v)| *k == key.as_str() && *v == value.as_str())
            })
    }
}

/// One block a player entered, for [`criteria_for_block`].
#[derive(Debug, Clone)]
pub struct BlockRecord {
    pub player: Uuid,
    pub block: &'static str,
    pub props: &'static [(&'static str, &'static str)],
}

/// Blocks players entered since the last drain.
pub static BLOCKS: Mutex<Vec<BlockRecord>> = Mutex::new(Vec::new());

type BlockSnapshot = (&'static str, &'static [(&'static str, &'static str)]);

/// Last block each player stood in, so entering is reported once and leaving re-arms it.
pub static LAST_BLOCK: Mutex<BTreeMap<Uuid, BlockSnapshot>> = Mutex::new(BTreeMap::new());

fn id_matches(list: &[String], kind: &str, id: &str) -> bool {
    let short = |s: &str| {
        s.strip_prefix("minecraft:")
            .map(str::to_owned)
            .unwrap_or_else(|| s.to_owned())
    };
    list.iter().any(|wanted| match wanted.strip_prefix('#') {
        Some(tag) => ferrite_data::tags::contains(kind, &short(tag), &short(id)),
        None => short(wanted) == short(id),
    })
}

impl UsedOnBlock {
    fn matches(&self, used: &UsedRecord) -> bool {
        (self.blocks.is_empty() || id_matches(&self.blocks, "block", used.block))
            && self
                .state
                .iter()
                .all(|(k, v)| used.props.iter().any(|(pk, pv)| pk == k && pv == v))
            && (self.items.is_empty()
                || used
                    .item
                    .is_some_and(|item| id_matches(&self.items, "item", item)))
            && self
                .biome
                .as_ref()
                .is_none_or(|wanted| used.biome.as_ref() == Some(wanted))
            && (!self.smokey || used.smokey)
            && (!self.playable
                || used
                    .item
                    .is_some_and(|item| item.starts_with("minecraft:music_disc_")))
    }
}

/// Parses `{"location": {"type": "all_of", "terms": [location_check, match_tool]}}`.
fn used_on_block(conditions: &serde_json::Value) -> Option<UsedOnBlock> {
    let object = conditions.as_object()?;
    let location = object.get("location")?;
    if object.len() != 1 {
        return None;
    }
    let terms = match location["type"].as_str()? {
        "minecraft:all_of" => location["terms"].as_array()?.clone(),
        _ => vec![location.clone()],
    };
    let mut out = UsedOnBlock::default();
    for term in &terms {
        let predicate = term["predicate"].as_object()?;
        match term["type"].as_str()? {
            "minecraft:location_check" => {
                for (key, value) in predicate {
                    match key.as_str() {
                        "block" => {
                            let block = value.as_object()?;
                            for (key, value) in block {
                                match key.as_str() {
                                    "blocks" => out.blocks = alternatives(value),
                                    "state" => {
                                        for (name, wanted) in value.as_object()? {
                                            out.state
                                                .push((name.clone(), wanted.as_str()?.to_owned()));
                                        }
                                    }
                                    _ => return None,
                                }
                            }
                        }
                        "biomes" => out.biome = Some(value.as_str()?.to_owned()),
                        "smokey" => out.smokey = value.as_bool()?,
                        _ => return None,
                    }
                }
            }
            "minecraft:match_tool" => {
                if predicate.len() != 1 {
                    return None;
                }
                if let Some(components) = predicate.get("predicates") {
                    // Only "can be played in a jukebox", which is what the music discs are.
                    let components = components.as_object()?;
                    if components.len() != 1
                        || !components.contains_key("minecraft:jukebox_playable")
                    {
                        return None;
                    }
                    out.playable = true;
                } else {
                    out.items = alternatives(predicate.get("items")?);
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Advancement criteria completed by using an item on a block.
pub fn criteria_for_used(used: &UsedRecord) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::UsedOnBlock(wanted) if wanted.matches(used)) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub recipes: Vec<String>,
    /// Experience points the advancement rewards.
    pub experience: i32,
    /// Loot tables rolled into the player's inventory (`rewards.loot`).
    pub loot: Vec<String>,
    /// Function run as the player (`rewards.function`).
    pub function: Option<String>,
    pub criteria: BTreeMap<String, Criterion>,
    pub requirements: Vec<Vec<String>>,
}

fn alternatives(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The entity type a `player_killed_entity` criterion asks for, when that is all it asks for.
fn kill_type(conditions: &serde_json::Value) -> Option<String> {
    let object = conditions.as_object()?;
    if object.keys().any(|k| k != "entity") {
        return None;
    }
    let entity = object.get("entity")?;
    let predicate = entity["predicate"].as_object()?;
    if entity["type"] != "minecraft:entity_properties"
        || entity["entity"] != "this"
        || predicate.len() != 1
    {
        return None;
    }
    let name = predicate.get("minecraft:entity_type")?.as_str()?;
    (!name.starts_with('#')).then(|| name.to_owned())
}

/// The item a `consume_item` criterion asks for (`*` for any), when that is all it asks for.
fn consumed_item(conditions: &serde_json::Value) -> Option<String> {
    if conditions.is_null() {
        return Some("*".to_owned());
    }
    let object = conditions.as_object()?;
    if object.is_empty() {
        return Some("*".to_owned());
    }
    let item = object.get("item")?.as_object()?;
    if object.len() != 1 || item.len() != 1 {
        return None;
    }
    let name = item.get("items")?.as_str()?;
    (!name.starts_with('#')).then(|| name.to_owned())
}

/// The block predicate of an `enter_block` criterion, when that is all it asks for.
fn entered_block(conditions: &serde_json::Value) -> Option<BlockMatch> {
    let object = conditions.as_object()?;
    // 26.3 puts `blocks`/`state` directly in the conditions; accept the older nested
    // `block` object too.
    let block = match object.get("block") {
        Some(serde_json::Value::Object(_)) => {
            if object.len() != 1 {
                return None;
            }
            object.get("block")?.as_object()?
        }
        _ => object,
    };
    if block.len() > 2 {
        return None;
    }
    let blocks = alternatives(block.get("blocks")?);
    if blocks.is_empty() {
        return None;
    }
    let mut state = Vec::new();
    if let Some(wanted) = block.get("state").and_then(|value| value.as_object()) {
        for (key, value) in wanted {
            state.push((key.clone(), value.as_str()?.to_owned()));
        }
    }
    Some(BlockMatch { blocks, state })
}

/// The block a `placed_block` criterion asks for, when that is all it asks for.
fn placed_block(conditions: &serde_json::Value) -> Option<String> {
    let object = conditions.as_object()?;
    let location = object.get("location")?.as_object()?;
    if object.len() != 1 || location.len() != 2 || location.get("type")? != "minecraft:match_block"
    {
        return None;
    }
    let name = location.get("blocks")?.as_str()?;
    (!name.starts_with('#')).then(|| name.to_owned())
}

/// The child a `bred_animals` criterion asks for (`*` for any), when that is all it asks for.
fn bred_child(conditions: &serde_json::Value) -> Option<String> {
    let object = match conditions.as_object() {
        None if conditions.is_null() => return Some("*".to_owned()),
        None => return None,
        Some(object) if object.is_empty() => return Some("*".to_owned()),
        Some(object) => object,
    };
    let child = object.get("child")?;
    let predicate = child["predicate"].as_object()?;
    if object.len() != 1 || child["type"] != "minecraft:entity_properties" || predicate.len() != 1 {
        return None;
    }
    let name = predicate.get("minecraft:entity_type")?.as_str()?;
    (!name.starts_with('#')).then(|| name.to_owned())
}

/// The biome a `location` criterion asks for, when that is all it asks for.
fn location_biome(conditions: &serde_json::Value) -> Option<String> {
    let object = conditions.as_object()?;
    let player = object.get("player")?;
    let predicate = player["predicate"].as_object()?;
    if object.len() != 1
        || player["type"] != "minecraft:entity_properties"
        || player["entity"] != "this"
        || predicate.len() != 1
    {
        return None;
    }
    let location = predicate.get("minecraft:location")?.as_object()?;
    if location.len() != 1 {
        return None;
    }
    let name = location.get("biomes")?.as_str()?;
    (!name.starts_with('#')).then(|| name.to_owned())
}

/// Recipes players crafted since the last drain, `(player, recipe id)`.
pub static CRAFTED: Mutex<Vec<(Uuid, String)>> = Mutex::new(Vec::new());

/// Advancement criteria completed by crafting a recipe.
pub fn criteria_for_crafted(recipe: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Crafts(wanted) if wanted == recipe) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// The recipe a `recipe_crafted` criterion asks for, when that is all it asks for.
fn crafted_recipe(conditions: &serde_json::Value) -> Option<String> {
    let object = conditions.as_object()?;
    (object.len() == 1).then_some(())?;
    Some(object.get("recipes")?.as_str()?.to_owned())
}

/// Biomes players stand in, `(player, biome id)`, pushed on every inventory scan.
pub static BIOMES: Mutex<Vec<(Uuid, String)>> = Mutex::new(Vec::new());

/// Advancement criteria completed by standing in a biome.
pub fn criteria_for_biome(biome: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::InBiome(wanted) if wanted == biome) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Advancement criteria completed by entering a block.
pub fn criteria_for_block(record: &BlockRecord) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if let Criterion::InBlock(wanted) = criterion
                && wanted.matches(record.block, record.props)
            {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Advancement criteria completed by the unconditioned `tick` trigger.
pub fn criteria_for_tick() -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Ticks) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Blocks players placed since the last drain, `(player, block id)`.
pub static PLACED: Mutex<Vec<(Uuid, &'static str)>> = Mutex::new(Vec::new());

/// Advancement criteria completed by placing a block.
pub fn criteria_for_placed(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Places(wanted) if wanted == block) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Babies players bred since the last drain, `(player, child entity type id)`.
pub static BRED: Mutex<Vec<(Uuid, &'static str)>> = Mutex::new(Vec::new());

/// Advancement criteria completed by breeding a child of this type.
pub fn criteria_for_bred(child: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Breeds(wanted) if wanted == "*" || wanted == child) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Players who tamed an animal since the last drain.
pub static TAMED: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

/// Advancement criteria completed by taming an animal.
pub fn criteria_for_tamed() -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Tames) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Items players finished eating or drinking since the last drain, `(player, item id)`.
pub static CONSUMED: Mutex<Vec<(Uuid, &'static str)>> = Mutex::new(Vec::new());

/// Advancement criteria completed by entering a dimension (`minecraft:the_end`, ...).
pub fn criteria_for_dimension(dimension: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::EntersDimension(to) if to == dimension) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Advancement criteria completed by consuming an item.
pub fn criteria_for_consumed(item: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Consumes(wanted) if wanted == "*" || wanted == item) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

/// Entity kinds players killed since the last drain, `(player, entity type id)`.
pub static KILLS: Mutex<Vec<(Uuid, &'static str)>> = Mutex::new(Vec::new());

/// Advancement criteria (`<advancement>#<criterion>`) completed by killing an entity type.
pub fn criteria_for_kill(entity: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        for (name, criterion) in &rule.criteria {
            if matches!(criterion, Criterion::Kills(kind) if kind == entity) {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

pub fn parse_rule(id: &str, json: &serde_json::Value) -> Option<Rule> {
    let recipes: Vec<String> = json["rewards"]["recipes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(str::to_owned))
        .collect();
    let experience = json["rewards"]["experience"]
        .as_i64()
        .and_then(|x| i32::try_from(x).ok())
        .unwrap_or(0);
    let loot: Vec<String> = json["rewards"]["loot"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().map(str::to_owned))
        .collect();
    let function = json["rewards"]["function"].as_str().map(str::to_owned);
    let mut criteria = BTreeMap::new();
    for (name, c) in json["criteria"].as_object()? {
        let kind = match c["trigger"].as_str() {
            Some("minecraft:inventory_changed") => Criterion::Holds(
                c["conditions"]["items"]
                    .as_array()
                    .map(|a| a.iter().map(|e| alternatives(&e["items"])).collect())
                    .unwrap_or_default(),
            ),
            Some("minecraft:enter_block") => {
                entered_block(&c["conditions"]).map_or(Criterion::Other, Criterion::InBlock)
            }
            Some("minecraft:tick")
                if c["conditions"].is_null()
                    || c["conditions"].as_object().is_some_and(|o| o.is_empty()) =>
            {
                Criterion::Ticks
            }
            Some("minecraft:recipe_unlocked") => {
                Criterion::Recipe(c["conditions"]["recipes"].as_str()?.to_owned())
            }
            Some("minecraft:placed_block") => {
                placed_block(&c["conditions"]).map_or(Criterion::Other, Criterion::Places)
            }
            Some("minecraft:changed_dimension") => {
                let conditions = c["conditions"].as_object();
                match conditions
                    .filter(|o| o.len() == 1)
                    .and_then(|o| o.get("to")?.as_str())
                {
                    Some(to) => Criterion::EntersDimension(to.to_owned()),
                    None => Criterion::Other,
                }
            }
            Some("minecraft:recipe_crafted") => {
                crafted_recipe(&c["conditions"]).map_or(Criterion::Other, Criterion::Crafts)
            }
            Some("minecraft:location") => {
                location_biome(&c["conditions"]).map_or(Criterion::Other, Criterion::InBiome)
            }
            Some("minecraft:tame_animal")
                if c["conditions"].is_null()
                    || c["conditions"].as_object().is_some_and(|o| o.is_empty()) =>
            {
                Criterion::Tames
            }
            Some("minecraft:item_used_on_block") => {
                used_on_block(&c["conditions"]).map_or(Criterion::Other, Criterion::UsedOnBlock)
            }
            Some("minecraft:bred_animals") => {
                bred_child(&c["conditions"]).map_or(Criterion::Other, Criterion::Breeds)
            }
            Some("minecraft:consume_item") => {
                consumed_item(&c["conditions"]).map_or(Criterion::Other, Criterion::Consumes)
            }
            Some("minecraft:player_killed_entity") => {
                kill_type(&c["conditions"]).map_or(Criterion::Other, Criterion::Kills)
            }
            _ => Criterion::Other,
        };
        criteria.insert(name.clone(), kind);
    }
    let requirements = match json["requirements"].as_array() {
        Some(groups) => groups.iter().map(alternatives).collect(),
        None => criteria.keys().map(|k| vec![k.clone()]).collect(),
    };
    Some(Rule {
        id: id.to_owned(),
        recipes,
        experience,
        loot,
        function,
        criteria,
        requirements,
    })
}

fn matches(entry: &[String], held: &BTreeSet<String>) -> bool {
    entry.iter().any(|alt| match alt.strip_prefix('#') {
        Some(tag) => {
            let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
            held.iter().any(|item| {
                ferrite_data::tags::contains(
                    "item",
                    tag,
                    item.strip_prefix("minecraft:").unwrap_or(item),
                )
            })
        }
        None => held.contains(alt),
    })
}

/// Recipes the rule gives to a player holding `held` who already has `owned`.
pub fn unlocked(rule: &Rule, held: &BTreeSet<String>, owned: &BTreeSet<String>) -> bool {
    let met = |name: &String| match rule.criteria.get(name) {
        Some(Criterion::Holds(entries)) => {
            !entries.is_empty() && entries.iter().all(|e| matches(e, held))
        }
        Some(Criterion::Recipe(id)) => owned.contains(id),
        _ => false,
    };
    !rule.requirements.is_empty() && rule.requirements.iter().all(|g| g.iter().any(met))
}

/// Reads the advancement rules now, off the caller's thread. The first `criteria_for_*` call
/// parses every advancement file, which stalled the coordinator for seconds when it did it.
pub fn preload() {
    std::thread::Builder::new()
        .name("ferrite-advancement-rules".into())
        .spawn(|| {
            rules();
        })
        .ok();
}

static ALL: OnceLock<Vec<Rule>> = OnceLock::new();

/// Whether the rules are loaded; the coordinator skips rule lookups until then instead of
/// waiting seconds for the first load.
pub fn ready() -> bool {
    ALL.get().is_some()
}

fn rules() -> &'static Vec<Rule> {
    ALL.get_or_init(|| {
        let relative = Path::new("vanilla/26.3/generated/data/minecraft/advancement");
        let Some(dir) = [
            relative.to_path_buf(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
        ]
        .into_iter()
        .find(|d| d.is_dir()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(current).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&dir) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                let Some(stem) = rel.strip_suffix(".json") else {
                    continue;
                };
                let id = format!("minecraft:{stem}");
                if let Some(rule) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .and_then(|j: serde_json::Value| parse_rule(&id, &j))
                {
                    out.push(rule);
                }
            }
        }
        out
    })
}

/// Adds the recipes `held` unlocks to `owned`; returns whether anything was added.
pub fn apply(held: &BTreeSet<String>, owned: &mut BTreeSet<String>) -> bool {
    let before = owned.len();
    // Repeat: a recipe given by one rule can satisfy another's `recipe_unlocked`.
    loop {
        let size = owned.len();
        for rule in rules() {
            if rule.recipes.is_empty() || rule.recipes.iter().all(|r| owned.contains(r)) {
                continue;
            }
            if unlocked(rule, held, owned) {
                owned.extend(rule.recipes.iter().cloned());
            }
        }
        if owned.len() == size {
            break;
        }
    }
    owned.len() != before
}

/// Recipes an advancement rewards when completed.
pub fn reward_recipes(advancement: &str) -> Vec<String> {
    rules()
        .iter()
        .find(|r| r.id == advancement)
        .map(|r| r.recipes.clone())
        .unwrap_or_default()
}

/// Experience points an advancement rewards when completed.
pub fn reward_experience(advancement: &str) -> i32 {
    rules()
        .iter()
        .find(|r| r.id == advancement)
        .map_or(0, |r| r.experience)
}

/// Loot tables an advancement rolls for the player when completed.
pub fn reward_loot(advancement: &str) -> Vec<String> {
    rules()
        .iter()
        .find(|r| r.id == advancement)
        .map(|r| r.loot.clone())
        .unwrap_or_default()
}

/// The function an advancement runs as the player when completed.
pub fn reward_function(advancement: &str) -> Option<String> {
    rules()
        .iter()
        .find(|r| r.id == advancement)
        .and_then(|r| r.function.clone())
}

/// Advancement criteria (`<advancement>#<criterion>`) that holding `held` completes, for every
/// advancement with an `inventory_changed` criterion except the recipe ones.
pub fn criteria_for(held: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for rule in rules() {
        if rule.id.starts_with("minecraft:recipes/") {
            continue;
        }
        for (name, criterion) in &rule.criteria {
            if let Criterion::Holds(entries) = criterion
                && !entries.is_empty()
                && entries.iter().all(|e| matches(e, held))
            {
                out.push(format!("{}#{name}", rule.id));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        parse_rule("t:stick", &serde_json::json!({
            "criteria": {
                "has_log": {"trigger": "minecraft:inventory_changed",
                    "conditions": {"items": [{"items": "#minecraft:logs"}]}},
                "has_planks": {"trigger": "minecraft:inventory_changed",
                    "conditions": {"items": [{"items": ["minecraft:oak_planks", "minecraft:birch_planks"]}]}},
                "has_the_recipe": {"trigger": "minecraft:recipe_unlocked",
                    "conditions": {"recipes": "minecraft:stick"}}
            },
            "requirements": [["has_the_recipe", "has_log", "has_planks"]],
            "rewards": {"recipes": ["minecraft:stick"], "experience": 25,
                "loot": ["t:chests/gift"], "function": "t:celebrate"}
        }))
        .unwrap()
    }

    #[test]
    fn enter_block_and_tick_criteria_parse() {
        let rule = parse_rule(
            "t:water",
            &serde_json::json!({
                "criteria": {
                    "in_water": {"trigger": "minecraft:enter_block",
                        "conditions": {"block": {"blocks": "minecraft:water"}}},
                    "every_tick": {"trigger": "minecraft:tick"}
                }
            }),
        )
        .unwrap();
        let Criterion::InBlock(block) = &rule.criteria["in_water"] else {
            panic!("{:?}", rule.criteria["in_water"]);
        };
        assert_eq!(block.blocks, ["minecraft:water"]);
        assert!(block.matches("minecraft:water", &[("level", "0")]));
        assert!(matches!(rule.criteria["every_tick"], Criterion::Ticks));
    }

    #[test]
    fn enter_block_and_tick_match_the_vanilla_advancements() {
        if rules().is_empty() {
            return;
        }
        assert!(criteria_for_tick().contains(
            &"minecraft:recipes/decorations/crafting_table#unlock_right_away".to_owned()
        ));
        let water = BlockRecord {
            player: Uuid::new_v4(),
            block: "minecraft:water",
            props: &[],
        };
        assert!(
            criteria_for_block(&water)
                .contains(&"minecraft:recipes/transportation/oak_boat#in_water".to_owned())
        );
    }

    #[test]
    fn a_held_item_or_the_recipe_itself_satisfies_the_rule() {
        let rule = rule();
        let held = |names: &[&str]| {
            names
                .iter()
                .map(|n| (*n).to_owned())
                .collect::<BTreeSet<_>>()
        };
        let none = BTreeSet::new();
        assert!(!unlocked(&rule, &held(&["minecraft:stone"]), &none));
        assert!(unlocked(&rule, &held(&["minecraft:birch_planks"]), &none));
        assert!(unlocked(&rule, &held(&["minecraft:oak_log"]), &none));
        assert!(unlocked(&rule, &none, &held(&["minecraft:stick"])));
    }

    #[test]
    fn criteria_for_skips_recipe_advancements() {
        let held = BTreeSet::from([
            "minecraft:oak_planks".to_owned(),
            "minecraft:iron_ingot".to_owned(),
        ]);
        assert!(
            criteria_for(&held)
                .iter()
                .all(|c| !c.starts_with("minecraft:recipes/"))
        );
    }

    #[test]
    fn plain_entity_kill_criteria_are_parsed_and_extra_conditions_are_not() {
        let plain = serde_json::json!({"entity": {"type": "minecraft:entity_properties",
            "entity": "this", "predicate": {"minecraft:entity_type": "minecraft:blaze"}}});
        assert_eq!(kill_type(&plain).as_deref(), Some("minecraft:blaze"));
        let tag = serde_json::json!({"entity": {"type": "minecraft:entity_properties",
            "entity": "this", "predicate": {"minecraft:entity_type": "#minecraft:skeletons"}}});
        assert_eq!(kill_type(&tag), None);
        let with_blow = serde_json::json!({"killing_blow": {}, "entity": plain["entity"]});
        assert_eq!(kill_type(&with_blow), None);
    }

    #[test]
    fn killing_the_dragon_completes_free_the_end() {
        let done = criteria_for_kill("minecraft:ender_dragon");
        assert_eq!(
            done.contains(&"minecraft:end/kill_dragon#killed_dragon".to_owned()),
            !rules().is_empty(),
            "{done:?}"
        );
    }

    #[test]
    fn killing_a_zombie_completes_monster_hunter_with_the_extracted_data() {
        let done = criteria_for_kill("minecraft:zombie");
        // The extracted data may be absent on this machine.
        assert_eq!(
            done.contains(&"minecraft:adventure/kill_a_mob#minecraft:zombie".to_owned()),
            !rules().is_empty()
        );
        assert!(criteria_for_kill("minecraft:cow").is_empty() || !rules().is_empty());
    }

    #[test]
    fn consume_criteria_name_one_item_or_any() {
        let one = serde_json::json!({"item": {"items": "minecraft:apple"}});
        assert_eq!(consumed_item(&one).as_deref(), Some("minecraft:apple"));
        assert_eq!(
            consumed_item(&serde_json::Value::Null).as_deref(),
            Some("*")
        );
        let tag = serde_json::json!({"item": {"items": "#minecraft:foods"}});
        assert_eq!(consumed_item(&tag), None);
        let counted = serde_json::json!({"item": {"items": "minecraft:apple", "count": 2}});
        assert_eq!(consumed_item(&counted), None);
    }

    #[test]
    fn entering_the_end_is_matched_with_the_extracted_data() {
        let done = criteria_for_dimension("minecraft:the_end");
        assert_eq!(
            done.contains(&"minecraft:story/enter_the_end#entered_end".to_owned()),
            !rules().is_empty()
        );
    }

    #[test]
    fn item_used_on_block_checks_block_state_tool_and_smoke() {
        let json = serde_json::json!({"location": {"type": "minecraft:all_of", "terms": [
            {"type": "minecraft:location_check", "predicate": {
                "block": {"blocks": "minecraft:respawn_anchor", "state": {"charges": "4"}}}},
            {"type": "minecraft:match_tool", "predicate": {"items": "minecraft:glowstone"}}]}});
        let wanted = used_on_block(&json).expect("parsed");
        let used = |charges: &'static str, item| UsedRecord {
            player: Uuid::nil(),
            block: "minecraft:respawn_anchor",
            props: Box::leak(Box::new([("charges", charges)])),
            item,
            biome: None,
            smokey: false,
        };
        assert!(wanted.matches(&used("4", Some("minecraft:glowstone"))));
        assert!(!wanted.matches(&used("3", Some("minecraft:glowstone"))));
        assert!(!wanted.matches(&used("4", Some("minecraft:stick"))));
        assert!(!wanted.matches(&used("4", None)));
        // "Playable in a jukebox" means a music disc; other component predicates are not guessed.
        let playable = |component: &str| {
            serde_json::json!({"location": {"type": "minecraft:all_of", "terms": [
                {"type": "minecraft:match_tool", "predicate": {"predicates": {component: {}}}}]}})
        };
        let jukebox = used_on_block(&playable("minecraft:jukebox_playable")).expect("parsed");
        let disc = |item| UsedRecord {
            player: Uuid::nil(),
            block: "minecraft:jukebox",
            props: &[],
            item,
            biome: None,
            smokey: false,
        };
        assert!(jukebox.matches(&disc(Some("minecraft:music_disc_13"))));
        assert!(!jukebox.matches(&disc(Some("minecraft:stick"))));
        assert!(used_on_block(&playable("minecraft:damage")).is_none());
    }

    #[test]
    fn bred_animals_criteria_match_any_child_or_one_child_type() {
        assert_eq!(bred_child(&serde_json::Value::Null).as_deref(), Some("*"));
        let cow = serde_json::json!({"child": {"type": "minecraft:entity_properties",
            "entity": "this", "predicate": {"minecraft:entity_type": "minecraft:cow"}}});
        assert_eq!(bred_child(&cow).as_deref(), Some("minecraft:cow"));
        let tag = serde_json::json!({"child": {"type": "minecraft:entity_properties",
            "entity": "this", "predicate": {"minecraft:entity_type": "#minecraft:x"}}});
        assert_eq!(bred_child(&tag), None);
        let parent = serde_json::json!({"parent": {}});
        assert_eq!(bred_child(&parent), None);
    }

    #[test]
    fn placed_block_criteria_match_one_plain_block() {
        let one = serde_json::json!({"location": {"type": "minecraft:match_block", "blocks": "minecraft:beetroots"}});
        assert_eq!(placed_block(&one).as_deref(), Some("minecraft:beetroots"));
        let tag = serde_json::json!({"location": {"type": "minecraft:match_block", "blocks": "#minecraft:beds"}});
        assert_eq!(placed_block(&tag), None);
        let with_state = serde_json::json!({"location": {"type": "minecraft:match_block", "blocks": "minecraft:x", "state": {}}});
        assert_eq!(placed_block(&with_state), None);
    }

    #[test]
    fn experience_rewards_are_parsed() {
        assert_eq!(rule().experience, 25);
        assert_eq!(rule().loot, vec!["t:chests/gift".to_owned()]);
        assert_eq!(rule().function.as_deref(), Some("t:celebrate"));
    }

    #[test]
    fn apply_unlocks_the_rewarded_recipes_once() {
        // The extracted data may be absent on this machine; with it, oak planks unlock recipes.
        let mut owned = BTreeSet::new();
        let held = BTreeSet::from(["minecraft:oak_planks".to_owned()]);
        let changed = apply(&held, &mut owned);
        assert_eq!(changed, !rules().is_empty());
        assert!(!apply(&held, &mut owned));
    }

    #[test]
    fn a_recipe_crafted_criterion_is_parsed_and_one_with_ingredients_is_not() {
        let json = |conditions| {
            serde_json::json!({"criteria": {"c": {"trigger": "minecraft:recipe_crafted",
                "conditions": conditions}}})
        };
        let rule = parse_rule(
            "a",
            &json(serde_json::json!({"recipes": "minecraft:stick"})),
        )
        .unwrap();
        assert!(matches!(&rule.criteria["c"], Criterion::Crafts(r) if r == "minecraft:stick"));
        let rule = parse_rule(
            "a",
            &json(serde_json::json!({"recipes": "minecraft:decorated_pot",
            "ingredients": [{"items": "minecraft:brick"}]})),
        )
        .unwrap();
        assert!(matches!(rule.criteria["c"], Criterion::Other));
    }

    #[test]
    fn a_biome_location_criterion_is_parsed() {
        let json = serde_json::json!({"criteria": {"badlands": {"trigger": "minecraft:location",
            "conditions": {"player": {"type": "minecraft:entity_properties", "entity": "this",
            "predicate": {"minecraft:location": {"biomes": "minecraft:badlands"}}}}}}});
        let rule = parse_rule("minecraft:adventure/adventuring_time", &json).unwrap();
        assert!(
            matches!(&rule.criteria["badlands"], Criterion::InBiome(b) if b == "minecraft:badlands")
        );
    }
}
