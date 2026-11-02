//! Vanilla Minecraft Java 26.3 recipe data and matching.
//!
//! [`RecipeBook`] loads the recipes and item tags from a generated data pack
//! directory at runtime. It understands shaped, shapeless and transmute
//! crafting, the furnace family, stonecutting, smithing transforms and trims,
//! and keeps every other recipe (including `crafting_special_*`) as a named
//! special recipe with its raw JSON available.
//!
//! Items and ingredients are plain strings, so this crate has no dependency on
//! the rest of Ferrite. Item ids are normalized to their `minecraft:` form:
//! `stick` and `minecraft:stick` name the same item.
//!
//! ```
//! use ferrite_recipe::RecipeBook;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let data = std::path::Path::new("vanilla/26.3/generated/data/minecraft");
//! if data.is_dir() {
//!     let book = RecipeBook::load(data)?;
//!     let mut grid = [None; 9];
//!     grid[4] = Some("minecraft:acacia_log");
//!     let recipe = book.match_crafting_3x3(&grid).expect("planks recipe");
//!     assert!(recipe.result().is_some());
//! }
//! # Ok(())
//! # }
//! ```

mod book;
pub mod brewing;
mod files;
mod matching;
mod recipe;
mod tags;

use std::path::Path;

pub use book::RecipeBook;
pub use recipe::{
    CookingKind, CookingRecipe, CraftingShaped, CraftingShapeless, CraftingTransmute, Ingredient,
    IngredientEntry, ItemStack, MaterialCount, Recipe, RecipeKind, SmithingTransform, SmithingTrim,
    SpecialRecipe, StonecuttingRecipe,
};
pub use tags::ItemTags;

/// Errors produced while loading recipes or tags.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A directory or file could not be read.
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// A JSON file was malformed.
    #[error("failed to parse {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    /// A recipe did not match the expected 26.3 shape.
    #[error("invalid recipe `{recipe}`: {message}")]
    Recipe { recipe: String, message: String },
    /// An item tag was malformed or referenced a missing required tag.
    #[error("invalid item tag `{tag}`: {message}")]
    Tag { tag: String, message: String },
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }

    pub(crate) fn json(path: &Path, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.display().to_string(),
            source,
        }
    }

    pub(crate) fn recipe(recipe: &str, message: impl Into<String>) -> Self {
        Self::Recipe {
            recipe: recipe.to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn tag(tag: &str, message: impl Into<String>) -> Self {
        Self::Tag {
            tag: tag.to_owned(),
            message: message.into(),
        }
    }
}

/// Result type used while loading recipe data.
pub type Result<T> = std::result::Result<T, Error>;
