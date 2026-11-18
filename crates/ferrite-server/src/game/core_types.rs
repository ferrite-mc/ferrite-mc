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
