//! Core-owned command, snapshot and wire value types shared by the game loop and regions.
//!
//! Host-only configuration and world-query types were removed with the embedded bridge;
//! this module now holds only values the server passes between its own systems.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
/// The metadata a plugin writes onto one stack: name and lore are text JSON, `custom_data` is the
/// persistent data container.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemMetaSpec {
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub enchants: Vec<(String, i32)>,
    pub unbreakable: bool,
    pub model: Option<i32>,
    pub repair: i32,
    pub flags: Vec<String>,
    pub custom_data: Option<ferrite_nbt::Compound>,
    pub book: Option<BookFields>,
    /// Components beyond the dedicated fields, keyed by full component name and carried in
    /// their 26.3 NBT shape (attributes, potion contents, glint override, limits, rarity, item
    /// model, tooltip style, use remainder, banner patterns, fireworks, profile, map colour).
    pub extra: Option<ferrite_nbt::Compound>,
}
/// The book a plugin writes onto a writable or written book (Bukkit `BookMeta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookFields {
    /// A signed book (`written_book`) rather than a book and quill (`writable_book`).
    pub written: bool,
    /// The title of a signed book.
    pub title: Option<String>,
    /// The author of a signed book.
    pub author: Option<String>,
    /// Copy generation: 0 original, 1 copy of the original, 2 copy of a copy, 3 tattered.
    pub generation: i32,
    /// One entry per page: text component JSON for a signed book, plain text for a writable one.
    pub pages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSnapshot {
    pub material: String,
    pub amount: u8,
}
/// What a plugin does to one of a player's shoulders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShoulderOp {
    /// Seats the parrot with this uuid there (lifting a live one out of the world).
    Seat(uuid::Uuid),
    /// Empties the shoulder without leaving a parrot behind (`setShoulderEntity*(null)`).
    Clear,
    /// Puts the parrot on that shoulder into the world beside the player.
    Release,
}
/// Stable identity for the main world published to plugins.
///
/// The Rust game addresses chunks by coordinate, so the Java side needs one id that both
/// `register_world` and every player location agree on.
pub fn default_world_id() -> uuid::Uuid {
    uuid::Uuid::from_u128(1)
}
