//! Argument parsers with their vanilla identifiers and wire properties.
//!
//! Each [`ArgumentType`] knows its parser identifier (the key in the
//! `minecraft:command_argument_type` registry), how to parse itself from a
//! [`StringReader`], its examples (for ambiguity detection), default suggestions, and its
//! `Commands`-packet properties. Parsers without a dedicated implementation are kept as
//! [`ArgumentType::Opaque`], so trees using them (vanilla or plugin) still serialize.

pub mod coords;
pub mod entity;
pub mod scan;

use ferrite_protocol::{Encode, VarInt};

use crate::error::{CommandSyntaxError, builtin, error_from, java_double, java_float, minecraft};
use crate::reader::StringReader;
use crate::suggestion::SuggestionsBuilder;

pub use coords::{Coordinates, WorldCoordinate};
pub use entity::{EntityTarget, ScoreHolder, SelectorKind, SelectorOption};

/// An entity anchor argument (`feet` or `eyes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityAnchor {
    Feet,
    Eyes,
}

impl EntityAnchor {
    pub const ALL: [EntityAnchor; 2] = [Self::Feet, Self::Eyes];

    pub fn name(self) -> &'static str {
        match self {
            Self::Feet => "feet",
            Self::Eyes => "eyes",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|anchor| anchor.name() == name)
    }

    /// Eye height above the entity's feet for a player.
    pub fn offset(self) -> f64 {
        match self {
            Self::Feet => 0.0,
            Self::Eyes => 1.62,
        }
    }
}

/// `brigadier:string` modes (the wire enum value is the discriminant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringKind {
    /// One unquoted word.
    SingleWord = 0,
    /// A word or a quoted phrase.
    QuotablePhrase = 1,
    /// The rest of the input.
    GreedyPhrase = 2,
}

/// Registry-backed parsers that carry a `registry` identifier property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryParser {
    ResourceOrTag,
    ResourceOrTagKey,
    Resource,
    ResourceKey,
    ResourceSelector,
}

impl RegistryParser {
    pub const ALL: [RegistryParser; 5] = [
        Self::ResourceOrTag,
        Self::ResourceOrTagKey,
        Self::Resource,
        Self::ResourceKey,
        Self::ResourceSelector,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::ResourceOrTag => "minecraft:resource_or_tag",
            Self::ResourceOrTagKey => "minecraft:resource_or_tag_key",
            Self::Resource => "minecraft:resource",
            Self::ResourceKey => "minecraft:resource_key",
            Self::ResourceSelector => "minecraft:resource_selector",
        }
    }

    fn allows_tags(self) -> bool {
        matches!(self, Self::ResourceOrTag | Self::ResourceOrTagKey)
    }
}

/// An argument parser and its properties.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentType {
    Bool,
    Float {
        min: f32,
        max: f32,
    },
    Double {
        min: f64,
        max: f64,
    },
    Integer {
        min: i32,
        max: i32,
    },
    Long {
        min: i64,
        max: i64,
    },
    String(StringKind),
    Entity {
        single: bool,
        players_only: bool,
    },
    GameProfile,
    BlockPos,
    ColumnPos,
    Vec3,
    Vec2,
    BlockState,
    Component,
    Message,
    ResourceLocation,
    Rotation,
    EntityAnchor,
    Dimension,
    Gamemode,
    Time {
        min: i32,
    },
    ScoreHolder {
        multiple: bool,
    },
    Registry {
        parser: RegistryParser,
        registry: String,
    },
    /// Any other parser, kept by identifier with pre-encoded wire properties. Parsing
    /// reads one bracket/quote-aware token.
    Opaque {
        parser: String,
        properties: Vec<u8>,
    },
}

/// A parsed argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentValue {
    Bool(bool),
    Float(f32),
    Double(f64),
    Integer(i32),
    Long(i64),
    String(String),
    Entity(EntityTarget),
    GameProfile(EntityTarget),
    BlockPos(Coordinates),
    Vec3(Coordinates),
    Vec2([WorldCoordinate; 2]),
    ColumnPos([WorldCoordinate; 2]),
    Rotation([WorldCoordinate; 2]),
    EntityAnchor(EntityAnchor),
    BlockState(BlockStateValue),
    /// Raw component source (JSON/SNBT), not interpreted.
    Component(String),
    Message(String),
    /// A normalized `namespace:path` identifier (also `dimension`).
    ResourceLocation(String),
    Resource {
        tag: bool,
        id: String,
    },
    Gamemode(GameMode),
    /// Ticks.
    Time(i32),
    ScoreHolder(ScoreHolder),
    /// Raw text of an opaque argument.
    Raw(String),
}

impl ArgumentValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Integer(v) | Self::Time(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(*v),
            Self::Integer(v) => Some(i64::from(*v)),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            Self::Float(v) => Some(f64::from(*v)),
            _ => None,
        }
    }

    /// Text of string-like values.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s)
            | Self::Message(s)
            | Self::Component(s)
            | Self::ResourceLocation(s)
            | Self::Raw(s) => Some(s),
            Self::Resource { id, .. } => Some(id),
            _ => None,
        }
    }

    pub fn as_entity(&self) -> Option<&EntityTarget> {
        match self {
            Self::Entity(t) | Self::GameProfile(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_coordinates(&self) -> Option<&Coordinates> {
        match self {
            Self::BlockPos(c) | Self::Vec3(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_gamemode(&self) -> Option<GameMode> {
        match self {
            Self::Gamemode(g) => Some(*g),
            _ => None,
        }
    }

    pub fn as_entity_anchor(&self) -> Option<EntityAnchor> {
        match self {
            Self::EntityAnchor(anchor) => Some(*anchor),
            _ => None,
        }
    }

    /// Whether this value is an entity selector (`@a`, `@e[...]`, `@s`, ...), which
    /// vanilla gates behind the `commands/entity_selectors` permission.
    pub fn has_selector(&self) -> bool {
        match self {
            Self::Entity(target) | Self::GameProfile(target) => target.is_selector(),
            Self::ScoreHolder(holder) => holder.is_selector(),
            _ => false,
        }
    }
}

/// `block_state` syntax: `id[key=value,...]{nbt}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStateValue {
    pub id: String,
    pub properties: Vec<(String, String)>,
    /// Raw SNBT including braces.
    pub nbt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub const ALL: [GameMode; 4] = [
        Self::Survival,
        Self::Creative,
        Self::Adventure,
        Self::Spectator,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
            Self::Adventure => "adventure",
            Self::Spectator => "spectator",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }
}

/// Vanilla's `RangeArgument.intRange` syntax: `n`, `n..`, `..n` or `a..b`, whole numbers only.
fn check_int_range(token: &str) -> Result<(), CommandSyntaxError> {
    let error = |key: &'static str, message: &str| {
        CommandSyntaxError::new(key, Vec::new(), message.to_owned())
    };
    let whole = |text: &str| text.parse::<i32>().is_ok();
    let (min, max) = match token.split_once("..") {
        Some((min, max)) => (min, max),
        None => (token, token),
    };
    if token.is_empty() || token == ".." {
        return Err(error(
            "argument.range.empty",
            "Expected value or range of values",
        ));
    }
    for part in [min, max].into_iter().filter(|part| !part.is_empty()) {
        if !whole(part) {
            return Err(error(
                "argument.range.ints",
                "Only whole numbers allowed, not decimals",
            ));
        }
    }
    if let (Ok(min), Ok(max)) = (min.parse::<i32>(), max.parse::<i32>())
        && min > max
    {
        return Err(error(
            "argument.range.swapped",
            "Min cannot be bigger than max",
        ));
    }
    Ok(())
}

fn check_range<T: PartialOrd + Copy>(
    reader: &mut StringReader,
    start: usize,
    value: T,
    (min, max): (T, T),
    fmt: impl Fn(T) -> String,
    low: fn(&StringReader, &str, &str) -> CommandSyntaxError,
    high: fn(&StringReader, &str, &str) -> CommandSyntaxError,
) -> Result<T, CommandSyntaxError> {
    if value < min {
        reader.set_cursor(start);
        Err(low(reader, &fmt(value), &fmt(min)))
    } else if value > max {
        reader.set_cursor(start);
        Err(high(reader, &fmt(value), &fmt(max)))
    } else {
        Ok(value)
    }
}

impl ArgumentType {
    pub fn bool() -> Self {
        Self::Bool
    }

    pub fn integer() -> Self {
        Self::integer_range(i32::MIN, i32::MAX)
    }

    pub fn integer_range(min: i32, max: i32) -> Self {
        Self::Integer { min, max }
    }

    pub fn long() -> Self {
        Self::Long {
            min: i64::MIN,
            max: i64::MAX,
        }
    }

    pub fn float() -> Self {
        Self::Float {
            min: -f32::MAX,
            max: f32::MAX,
        }
    }

    pub fn double() -> Self {
        Self::Double {
            min: -f64::MAX,
            max: f64::MAX,
        }
    }

    pub fn word() -> Self {
        Self::String(StringKind::SingleWord)
    }

    pub fn string() -> Self {
        Self::String(StringKind::QuotablePhrase)
    }

    pub fn greedy_string() -> Self {
        Self::String(StringKind::GreedyPhrase)
    }

    pub fn entities() -> Self {
        Self::Entity {
            single: false,
            players_only: false,
        }
    }

    pub fn entity() -> Self {
        Self::Entity {
            single: true,
            players_only: false,
        }
    }

    pub fn players() -> Self {
        Self::Entity {
            single: false,
            players_only: true,
        }
    }

    pub fn player() -> Self {
        Self::Entity {
            single: true,
            players_only: true,
        }
    }

    /// An opaque parser with no properties.
    pub fn opaque(parser: impl Into<String>) -> Self {
        Self::Opaque {
            parser: parser.into(),
            properties: Vec::new(),
        }
    }

    /// The `minecraft:command_argument_type` registry key.
    pub fn parser_id(&self) -> &str {
        match self {
            Self::Bool => "brigadier:bool",
            Self::Float { .. } => "brigadier:float",
            Self::Double { .. } => "brigadier:double",
            Self::Integer { .. } => "brigadier:integer",
            Self::Long { .. } => "brigadier:long",
            Self::String(_) => "brigadier:string",
            Self::Entity { .. } => "minecraft:entity",
            Self::GameProfile => "minecraft:game_profile",
            Self::BlockPos => "minecraft:block_pos",
            Self::ColumnPos => "minecraft:column_pos",
            Self::Vec3 => "minecraft:vec3",
            Self::Vec2 => "minecraft:vec2",
            Self::BlockState => "minecraft:block_state",
            Self::Component => "minecraft:component",
            Self::Message => "minecraft:message",
            Self::ResourceLocation => "minecraft:resource_location",
            Self::Rotation => "minecraft:rotation",
            Self::EntityAnchor => "minecraft:entity_anchor",
            Self::Dimension => "minecraft:dimension",
            Self::Gamemode => "minecraft:gamemode",
            Self::Time { .. } => "minecraft:time",
            Self::ScoreHolder { .. } => "minecraft:score_holder",
            Self::Registry { parser, .. } => parser.id(),
            Self::Opaque { parser, .. } => parser,
        }
    }

    /// Parses one argument starting at the reader's cursor.
    pub fn parse(&self, reader: &mut StringReader) -> Result<ArgumentValue, CommandSyntaxError> {
        let start = reader.cursor();
        Ok(match self {
            Self::Bool => ArgumentValue::Bool(reader.read_boolean()?),
            Self::Integer { min, max } => {
                let v = reader.read_int()?;
                ArgumentValue::Integer(check_range(
                    reader,
                    start,
                    v,
                    (*min, *max),
                    |v| v.to_string(),
                    builtin::integer_too_low,
                    builtin::integer_too_high,
                )?)
            }
            Self::Long { min, max } => {
                let v = reader.read_long()?;
                ArgumentValue::Long(check_range(
                    reader,
                    start,
                    v,
                    (*min, *max),
                    |v| v.to_string(),
                    builtin::long_too_low,
                    builtin::long_too_high,
                )?)
            }
            Self::Float { min, max } => {
                let v = reader.read_float()?;
                ArgumentValue::Float(check_range(
                    reader,
                    start,
                    v,
                    (*min, *max),
                    java_float,
                    builtin::float_too_low,
                    builtin::float_too_high,
                )?)
            }
            Self::Double { min, max } => {
                let v = reader.read_double()?;
                ArgumentValue::Double(check_range(
                    reader,
                    start,
                    v,
                    (*min, *max),
                    java_double,
                    builtin::double_too_low,
                    builtin::double_too_high,
                )?)
            }
            Self::String(kind) => ArgumentValue::String(match kind {
                StringKind::SingleWord => reader.read_unquoted_string().to_owned(),
                StringKind::QuotablePhrase => reader.read_string()?,
                StringKind::GreedyPhrase => reader.read_rest().to_owned(),
            }),
            Self::Entity {
                single,
                players_only,
            } => ArgumentValue::Entity(entity::parse_entity(reader, *single, *players_only)?),
            Self::GameProfile => ArgumentValue::GameProfile(entity::parse_game_profile(reader)?),
            Self::BlockPos => ArgumentValue::BlockPos(coords::parse_block_pos(reader)?),
            Self::Vec3 => ArgumentValue::Vec3(coords::parse_vec3(reader, true)?),
            Self::Vec2 => ArgumentValue::Vec2(coords::parse_pair(reader, false, true)?),
            Self::ColumnPos => ArgumentValue::ColumnPos(coords::parse_pair(reader, true, false)?),
            Self::Rotation => ArgumentValue::Rotation(coords::parse_pair(reader, false, false)?),
            Self::EntityAnchor => {
                let name = reader.read_unquoted_string().to_owned();
                ArgumentValue::EntityAnchor(
                    EntityAnchor::from_name(&name)
                        .ok_or_else(|| minecraft::anchor_invalid(reader, &name))?,
                )
            }
            Self::BlockState => ArgumentValue::BlockState(parse_block_state(reader)?),
            Self::Component => ArgumentValue::Component(scan::read_token(reader)?),
            Self::Message => ArgumentValue::Message(reader.read_rest().to_owned()),
            Self::ResourceLocation | Self::Dimension => {
                ArgumentValue::ResourceLocation(scan::read_identifier(reader)?)
            }
            Self::Gamemode => {
                let name = reader.read_unquoted_string().to_owned();
                ArgumentValue::Gamemode(
                    GameMode::from_name(&name)
                        .ok_or_else(|| minecraft::gamemode_invalid(reader, &name))?,
                )
            }
            Self::Time { min } => ArgumentValue::Time(parse_time(reader, *min)?),
            Self::ScoreHolder { multiple } => {
                ArgumentValue::ScoreHolder(entity::parse_score_holder(reader, *multiple)?)
            }
            Self::Registry { parser, .. } => {
                if *parser == RegistryParser::ResourceSelector {
                    ArgumentValue::Resource {
                        tag: false,
                        id: reader.read_until_separator().to_owned(),
                    }
                } else {
                    let tag = parser.allows_tags() && reader.peek_is('#');
                    if tag {
                        reader.skip();
                    }
                    let id = scan::read_identifier(reader)?;
                    ArgumentValue::Resource { tag, id }
                }
            }
            Self::Opaque { parser, .. } => {
                let start = reader.cursor();
                let token = scan::read_token(reader)?;
                if parser == "minecraft:int_range"
                    && let Err(error) = check_int_range(&token)
                {
                    reader.set_cursor(start);
                    return Err(error.with_context(reader));
                }
                ArgumentValue::Raw(token)
            }
        })
    }

    /// Example inputs, used for ambiguity detection (Brigadier's `getExamples`).
    pub fn examples(&self) -> &'static [&'static str] {
        match self {
            Self::Bool => &["true", "false"],
            Self::Integer { .. } | Self::Long { .. } => &["0", "123", "-123"],
            Self::Float { .. } | Self::Double { .. } => {
                &["0", "1.2", ".5", "-1", "-.5", "-1234.56"]
            }
            Self::String(StringKind::SingleWord) => &["word", "words_with_underscores"],
            Self::String(StringKind::QuotablePhrase) => &["\"quoted phrase\"", "word", "\"\""],
            Self::String(StringKind::GreedyPhrase) => {
                &["word", "words with spaces", "\"and symbols\""]
            }
            Self::Entity { .. } => &[
                "Player",
                "0123",
                "@e",
                "@e[type=foo]",
                "dd12be42-52a9-4a91-a8a1-11c01849e498",
            ],
            Self::GameProfile => &[
                "Player",
                "0123",
                "dd12be42-52a9-4a91-a8a1-11c01849e498",
                "@e",
            ],
            Self::BlockPos => &["0 0 0", "~ ~ ~", "^ ^ ^", "^1 ^ ^-5", "~0.5 ~1 ~-5"],
            Self::Vec3 => &["0 0 0", "~ ~ ~", "^ ^ ^", "^1 ^ ^-5", "0.1 -0.5 .9"],
            Self::Vec2 => &["0 0", "~ ~", "0.1 -0.5", "~1 ~-2"],
            Self::ColumnPos => &["0 0", "~ ~", "~1 ~-2"],
            Self::Rotation => &["0 0", "~ ~", "~-5 ~5"],
            Self::EntityAnchor => &["feet", "eyes"],
            Self::BlockState => &["stone", "minecraft:stone", "stone[foo=bar]", "foo{bar=baz}"],
            Self::Component => &["\"hello world\"", "\"\"", "{text:\"hello world\"}"],
            Self::Message => &["Hello world!", "foo", "@e", "Hello @p :)"],
            Self::ResourceLocation => &["foo", "foo:bar", "012"],
            Self::Dimension => &["minecraft:overworld", "minecraft:the_nether"],
            Self::Gamemode => &["survival", "creative"],
            Self::Time { .. } => &["0d", "0s", "0t", "0"],
            Self::ScoreHolder { .. } => &["Player", "0123", "*", "@e"],
            Self::Registry { parser, .. } if parser.allows_tags() => &[
                "foo",
                "foo:bar",
                "012",
                "#skeletons",
                "#minecraft:skeletons",
            ],
            Self::Registry { .. } => &["foo", "foo:bar", "012"],
            Self::Opaque { .. } => &[],
        }
    }

    /// Suggestions the parser can make without game state (Brigadier's
    /// `listSuggestions` default for the type).
    pub fn list_suggestions(&self, builder: &mut SuggestionsBuilder) {
        let remaining = builder.remaining().to_owned();
        match self {
            Self::Bool => {
                builder.suggest_matching("true").suggest_matching("false");
            }
            Self::Gamemode => {
                for mode in GameMode::ALL {
                    builder.suggest_matching(mode.name());
                }
            }
            Self::EntityAnchor => {
                for anchor in EntityAnchor::ALL {
                    builder.suggest_matching(anchor.name());
                }
            }
            Self::Entity { .. } | Self::GameProfile | Self::ScoreHolder { .. }
                if remaining.is_empty() || remaining.starts_with('@') =>
            {
                for kind in SelectorKind::ALL {
                    builder.suggest_matching(format!("@{}", kind.char()));
                }
            }
            Self::BlockPos | Self::Vec3 if remaining.is_empty() => {
                builder.suggest("~").suggest("~ ~").suggest("~ ~ ~");
            }
            Self::Vec2 | Self::ColumnPos | Self::Rotation if remaining.is_empty() => {
                builder.suggest("~").suggest("~ ~");
            }
            _ => {}
        }
    }

    /// Appends the `Commands`-packet properties for this parser.
    pub fn write_properties(&self, out: &mut Vec<u8>) {
        fn flags(has_min: bool, has_max: bool) -> u8 {
            u8::from(has_min) | (u8::from(has_max) << 1)
        }
        match self {
            Self::Float { min, max } => {
                let (a, b) = (*min != -f32::MAX, *max != f32::MAX);
                out.push(flags(a, b));
                if a {
                    min.encode(out);
                }
                if b {
                    max.encode(out);
                }
            }
            Self::Double { min, max } => {
                let (a, b) = (*min != -f64::MAX, *max != f64::MAX);
                out.push(flags(a, b));
                if a {
                    min.encode(out);
                }
                if b {
                    max.encode(out);
                }
            }
            Self::Integer { min, max } => {
                let (a, b) = (*min != i32::MIN, *max != i32::MAX);
                out.push(flags(a, b));
                if a {
                    min.encode(out);
                }
                if b {
                    max.encode(out);
                }
            }
            Self::Long { min, max } => {
                let (a, b) = (*min != i64::MIN, *max != i64::MAX);
                out.push(flags(a, b));
                if a {
                    min.encode(out);
                }
                if b {
                    max.encode(out);
                }
            }
            Self::String(kind) => VarInt(*kind as i32).encode(out),
            Self::Entity {
                single,
                players_only,
            } => out.push(u8::from(*single) | (u8::from(*players_only) << 1)),
            Self::ScoreHolder { multiple } => out.push(u8::from(*multiple)),
            Self::Time { min } => min.encode(out),
            Self::Registry { registry, .. } => registry.encode(out),
            Self::Opaque { properties, .. } => out.extend_from_slice(properties),
            _ => {}
        }
    }

    /// Builds a type from its identifier and the `properties` object of a vanilla
    /// `commands.json` node. Unknown identifiers become [`ArgumentType::Opaque`]; returns
    /// an error message if the properties are malformed or cannot be encoded.
    pub fn from_json(
        parser: &str,
        props: Option<&serde_json::Value>,
    ) -> Result<(Self, bool), String> {
        use serde_json::Value;
        let get = |key: &str| props.and_then(|p| p.get(key));
        let num = |key: &str| get(key).and_then(Value::as_f64);
        let int = |key: &str| get(key).and_then(Value::as_i64);
        let text = |key: &str| -> Result<&str, String> {
            get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{parser}: missing string property `{key}`"))
        };
        let ty = match parser {
            "brigadier:bool" => Self::Bool,
            "brigadier:float" => Self::Float {
                min: num("min").map_or(-f32::MAX, |v| v as f32),
                max: num("max").map_or(f32::MAX, |v| v as f32),
            },
            "brigadier:double" => Self::Double {
                min: num("min").unwrap_or(-f64::MAX),
                max: num("max").unwrap_or(f64::MAX),
            },
            "brigadier:integer" => Self::Integer {
                min: int("min").map_or(i32::MIN, |v| v as i32),
                max: int("max").map_or(i32::MAX, |v| v as i32),
            },
            "brigadier:long" => Self::Long {
                min: int("min").unwrap_or(i64::MIN),
                max: int("max").unwrap_or(i64::MAX),
            },
            "brigadier:string" => Self::String(match text("type")? {
                "word" => StringKind::SingleWord,
                "phrase" => StringKind::QuotablePhrase,
                "greedy" => StringKind::GreedyPhrase,
                other => return Err(format!("brigadier:string: unknown type `{other}`")),
            }),
            "minecraft:entity" => Self::Entity {
                single: text("amount")? == "single",
                players_only: text("type")? == "players",
            },
            "minecraft:score_holder" => Self::ScoreHolder {
                multiple: text("amount")? == "multiple",
            },
            "minecraft:time" => Self::Time {
                min: int("min").map_or(0, |v| v as i32),
            },
            "minecraft:game_profile" => Self::GameProfile,
            "minecraft:block_pos" => Self::BlockPos,
            "minecraft:column_pos" => Self::ColumnPos,
            "minecraft:vec3" => Self::Vec3,
            "minecraft:vec2" => Self::Vec2,
            "minecraft:block_state" => Self::BlockState,
            "minecraft:component" => Self::Component,
            "minecraft:message" => Self::Message,
            "minecraft:resource_location" => Self::ResourceLocation,
            "minecraft:rotation" => Self::Rotation,
            "minecraft:entity_anchor" => Self::EntityAnchor,
            "minecraft:dimension" => Self::Dimension,
            "minecraft:gamemode" => Self::Gamemode,
            other => {
                if let Some(parser) = RegistryParser::ALL.into_iter().find(|p| p.id() == other) {
                    Self::Registry {
                        parser,
                        registry: text("registry")?.to_owned(),
                    }
                } else {
                    let has_props = props.is_some_and(|p| !p.is_null());
                    if has_props {
                        return Err(format!(
                            "{other}: properties of opaque parser not encodable"
                        ));
                    }
                    return Ok((Self::opaque(other), true));
                }
            }
        };
        Ok((ty, false))
    }
}

fn parse_time(reader: &mut StringReader, min: i32) -> Result<i32, CommandSyntaxError> {
    let amount = reader.read_float()?;
    let unit = reader.read_unquoted_string().to_owned();
    let multiplier = match unit.as_str() {
        "" | "t" => 1.0,
        "s" => 20.0,
        "d" => 24000.0,
        _ => return Err(minecraft::time_invalid_unit(reader)),
    };
    // Java's Math.round(float): floor(x + 0.5), saturating.
    let ticks = (amount * multiplier + 0.5).floor() as i32;
    if ticks < min {
        return Err(minecraft::time_too_low(reader, ticks, min));
    }
    Ok(ticks)
}

fn parse_block_state(reader: &mut StringReader) -> Result<BlockStateValue, CommandSyntaxError> {
    let id = scan::read_identifier(reader)?;
    let mut properties = Vec::new();
    if reader.peek_is('[') {
        let start = reader.cursor();
        scan::read_balanced(reader)?;
        let inner = reader.string()[start + 1..reader.cursor() - 1].to_owned();
        for entry in inner.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let Some((key, value)) = entry.split_once('=') else {
                reader.set_cursor(start);
                return Err(error_from(
                    "argument.block.property.novalue",
                    vec![entry.to_owned(), id.clone()],
                    format!("Expected value for property '{entry}' on block {id}"),
                    reader,
                ));
            };
            properties.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let nbt = if reader.peek_is('{') {
        let start = reader.cursor();
        scan::read_balanced(reader)?;
        Some(reader.string()[start..reader.cursor()].to_owned())
    } else {
        None
    };
    Ok(BlockStateValue {
        id,
        properties,
        nbt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(ty: &ArgumentType, s: &str) -> Result<ArgumentValue, CommandSyntaxError> {
        ty.parse(&mut StringReader::new(s.to_owned()))
    }

    #[test]
    fn int_range_arguments_validate_at_parse_time() {
        let parse = |input: &str| {
            let mut reader = StringReader::new(input.to_owned());
            ArgumentType::opaque("minecraft:int_range").parse(&mut reader)
        };
        for good in ["5", "-3", "1..", "..7", "2..9", "4..4"] {
            assert_eq!(
                parse(good).unwrap(),
                ArgumentValue::Raw(good.to_owned()),
                "{good}"
            );
        }
        for (bad, key) in [
            ("7..3", "argument.range.swapped"),
            ("1.5", "argument.range.ints"),
            ("a..b", "argument.range.ints"),
            ("..", "argument.range.empty"),
        ] {
            assert_eq!(parse(bad).unwrap_err().translation_key(), key, "{bad}");
        }
        // Other opaque parsers stay unchecked.
        let mut reader = StringReader::new("7..3".to_owned());
        assert!(
            ArgumentType::opaque("minecraft:float_range")
                .parse(&mut reader)
                .is_ok()
        );
    }

    #[test]
    fn integer_range_errors() {
        let ty = ArgumentType::integer_range(1, 64);
        assert_eq!(parse(&ty, "64").unwrap(), ArgumentValue::Integer(64));
        let low = parse(&ty, "0").unwrap_err();
        assert_eq!(low.message(), "Integer must not be less than 1: found 0");
        assert_eq!(low.args(), ["1", "0"]);
        assert_eq!(low.cursor(), Some(0));
        let high = parse(&ty, "65").unwrap_err();
        assert_eq!(high.message(), "Integer must not be more than 64: found 65");
        assert_eq!(high.translation_key(), "argument.integer.big");
    }

    #[test]
    fn float_and_double_ranges_format_like_java() {
        let ty = ArgumentType::Double {
            min: -59999968.0,
            max: 59999968.0,
        };
        assert_eq!(
            parse(&ty, "60000000").unwrap_err().message(),
            "Double must not be more than 5.9999968E7: found 6.0E7"
        );
        let ty = ArgumentType::Float { min: 0.0, max: 1.0 };
        assert_eq!(
            parse(&ty, "-1").unwrap_err().message(),
            "Float must not be less than 0.0: found -1.0"
        );
    }

    #[test]
    fn string_kinds() {
        let mut r = StringReader::new("hello world");
        assert_eq!(
            ArgumentType::word().parse(&mut r).unwrap(),
            ArgumentValue::String("hello".into())
        );
        assert_eq!(
            parse(&ArgumentType::string(), r#""a \"b\"" c"#).unwrap(),
            ArgumentValue::String(r#"a "b""#.into())
        );
        assert_eq!(
            parse(&ArgumentType::greedy_string(), "all of \"this\" ").unwrap(),
            ArgumentValue::String("all of \"this\" ".into())
        );
    }

    #[test]
    fn minecraft_simple_types() {
        assert_eq!(
            parse(&ArgumentType::Time { min: 0 }, "1.5d").unwrap(),
            ArgumentValue::Time(36000)
        );
        assert_eq!(
            parse(&ArgumentType::Time { min: 0 }, "2x")
                .unwrap_err()
                .message(),
            "Invalid unit"
        );
        assert_eq!(
            parse(&ArgumentType::Time { min: 1 }, "0")
                .unwrap_err()
                .message(),
            "The tick count must not be less than 1: found 0"
        );
        assert_eq!(
            parse(&ArgumentType::Gamemode, "creative").unwrap(),
            ArgumentValue::Gamemode(GameMode::Creative)
        );
        assert_eq!(
            parse(&ArgumentType::Gamemode, "god").unwrap_err().message(),
            "Unknown game mode: god"
        );
        assert_eq!(
            parse(&ArgumentType::EntityAnchor, "eyes").unwrap(),
            ArgumentValue::EntityAnchor(EntityAnchor::Eyes)
        );
        let anchor = parse(&ArgumentType::EntityAnchor, "head").unwrap_err();
        assert_eq!(anchor.translation_key(), "argument.anchor.invalid");
        assert_eq!(anchor.message(), "Invalid entity anchor position head");
        assert_eq!(
            parse(&ArgumentType::Dimension, "the_nether").unwrap(),
            ArgumentValue::ResourceLocation("minecraft:the_nether".into())
        );
        let tagged = ArgumentType::Registry {
            parser: RegistryParser::ResourceOrTag,
            registry: "minecraft:worldgen/biome".into(),
        };
        assert_eq!(
            parse(&tagged, "#minecraft:is_ocean").unwrap(),
            ArgumentValue::Resource {
                tag: true,
                id: "minecraft:is_ocean".into()
            }
        );
    }

    #[test]
    fn block_state_shape() {
        let v = parse(
            &ArgumentType::BlockState,
            "oak_stairs[facing=north, half=top]{a:1} x",
        )
        .unwrap();
        assert_eq!(
            v,
            ArgumentValue::BlockState(BlockStateValue {
                id: "minecraft:oak_stairs".into(),
                properties: vec![
                    ("facing".into(), "north".into()),
                    ("half".into(), "top".into())
                ],
                nbt: Some("{a:1}".into()),
            })
        );
    }

    #[test]
    fn opaque_and_component_tokens() {
        assert_eq!(
            parse(&ArgumentType::opaque("minecraft:nbt_path"), "a.b[0] rest").unwrap(),
            ArgumentValue::Raw("a.b[0]".into())
        );
        assert_eq!(
            parse(&ArgumentType::Component, r#"{"text":"hi there"} 1"#).unwrap(),
            ArgumentValue::Component(r#"{"text":"hi there"}"#.into())
        );
    }

    #[test]
    fn properties_encoding() {
        let mut out = Vec::new();
        ArgumentType::integer().write_properties(&mut out);
        assert_eq!(out, [0]);
        out.clear();
        ArgumentType::integer_range(1, 5).write_properties(&mut out);
        assert_eq!(out, [3, 0, 0, 0, 1, 0, 0, 0, 5]);
        out.clear();
        ArgumentType::player().write_properties(&mut out);
        assert_eq!(out, [3]);
        out.clear();
        ArgumentType::greedy_string().write_properties(&mut out);
        assert_eq!(out, [2]);
    }
}
