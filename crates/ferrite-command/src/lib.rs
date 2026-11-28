//! Ferrite's command system: a game-agnostic, Brigadier-compatible dispatcher.
//!
//! - [`CommandDispatcher`] owns a node graph (root / literal / argument nodes) generic over
//!   the command source `S`, with per-node requirements, redirects and forks.
//! - [`StringReader`] and [`arguments`] parse input exactly like Brigadier and vanilla,
//!   with vanilla's error messages and translation keys.
//! - [`CommandDispatcher::completion_suggestions`] does tab completion at a cursor.
//! - [`CommandDispatcher::serialize_commands`] encodes the clientbound `Commands` packet
//!   body for one source, using a [`ParserRegistry`] read from vanilla's `registries.json`.
//! - [`CommandDispatcher::load_vanilla_json`] builds the vanilla tree from the data
//!   generator's `commands.json`.
//!
//! ```
//! use ferrite_command::{ArgumentType, CommandDispatcher, argument, literal};
//!
//! let mut d = CommandDispatcher::<()>::new();
//! d.register(literal("add").then(
//!     argument("a", ArgumentType::integer()).then(
//!         argument("b", ArgumentType::integer())
//!             .executes(|ctx| Ok(ctx.integer("a").unwrap() + ctx.integer("b").unwrap())),
//!     ),
//! ));
//! assert_eq!(d.execute("add 2 3", ()).unwrap(), 5);
//! ```

pub mod arguments;
pub mod context;
pub mod dispatcher;
pub mod error;
pub mod packet;
pub mod reader;
pub mod suggestion;
pub mod tree;
pub mod vanilla;

pub use arguments::{ArgumentType, ArgumentValue, EntityAnchor, StringKind};
pub use context::{CommandContext, ParsedArgument, ParsedNode};
pub use dispatcher::{Ambiguity, CommandDispatcher, ParseResults};
pub use error::{CommandError, CommandSyntaxError, ErrorContext};
pub use packet::{ParserRegistry, SerializeError, SerializedCommands};
pub use reader::StringReader;
pub use suggestion::{StringRange, Suggestion, Suggestions, SuggestionsBuilder};
pub use tree::{
    Command, Node, NodeBuilder, NodeId, NodeKind, RedirectModifier, Requirement, Suggester,
    SuggestionKind, SuggestionProvider, argument, literal,
};
pub use vanilla::{
    CommandFactory, LoadError, LoadOptions, LoadReport, PermissionLevel, PermissionSource,
    RequirementFactory,
};

pub mod gamerule;
pub mod limits;
