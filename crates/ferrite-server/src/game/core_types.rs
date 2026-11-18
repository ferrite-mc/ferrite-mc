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
