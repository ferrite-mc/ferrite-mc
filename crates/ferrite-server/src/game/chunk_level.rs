//! Vanilla chunk levels and player tickets (`ChunkLevel`, `DistanceManager`, `ChunkMap`),
//! checked against the 26.3 server bytecode.
//!
//! - `ChunkLevel`: a chunk is fully loaded at level `<= 33`, block ticking at `<= 32` and entity
//!   ticking at `<= 31`; `MAX_LEVEL` is `33 + RADIUS_AROUND_FULL_CHUNK`.
//! - `DistanceManager.PLAYER_TICKET_LEVEL` is the entity-ticking level (31). A player holds a
//!   `PLAYER_LOADING` ticket (no timeout) on every chunk within `viewDistance` (Chebyshev) of
//!   the chunk they stand in, and a `PLAYER_SIMULATION` ticket of level
//!   `max(0, 31 - simulationDistance)` on their own chunk. A ticket's level grows by one for every
//!   chunk away from it.
//! - `ChunkMap.setServerViewDistance` clamps the server view distance to `2..=32`;
//!   `getPlayerViewDistance` clamps the distance the client asked for to `2..=server`.
//!   `ChunkTrackingView` (what the client is sent) is the square of that radius.
//! - `ChunkMap.processUnloads` drops every chunk whose level rose above 33 on the next tick and
//!   saves it; there is no grace period for a chunk a player left.
//!
//! What stays different on purpose: Ferrite loads and holds exactly the square the client is sent
//! (no extra two-chunk ring of lower-status chunks), because chunks are held per region viewer
//! rather than through a level graph, and ticking follows the held chunks instead of the
//! simulation distance.

/// `ChunkLevel.FULL_CHUNK_LEVEL`.
pub const FULL_LEVEL: i32 = 33;
/// `ChunkLevel.BLOCK_TICKING_LEVEL`.
pub const BLOCK_TICKING_LEVEL: i32 = 32;
/// `ChunkLevel.ENTITY_TICKING_LEVEL`, also `DistanceManager.PLAYER_TICKET_LEVEL`.
pub const ENTITY_TICKING_LEVEL: i32 = 31;

/// `ChunkLevel.isLoaded`.
pub fn is_loaded(level: i32) -> bool {
    level <= FULL_LEVEL
}

/// `ChunkLevel.isBlockTicking`.
pub fn is_block_ticking(level: i32) -> bool {
    level <= BLOCK_TICKING_LEVEL
}

/// `ChunkLevel.isEntityTicking`.
pub fn is_entity_ticking(level: i32) -> bool {
    level <= ENTITY_TICKING_LEVEL
}

/// The level of a `PLAYER_SIMULATION` ticket (`DistanceManager.getPlayerTicketLevel`).
pub fn simulation_ticket_level(simulation_distance: i32) -> i32 {
    (ENTITY_TICKING_LEVEL - simulation_distance).max(0)
}

/// A chunk `distance` chunks (Chebyshev) from a ticket of `ticket_level`.
pub fn level_at(ticket_level: i32, distance: i32) -> i32 {
    ticket_level + distance
}

/// `ChunkMap.setServerViewDistance`.
pub fn server_view(configured: i32) -> i32 {
    configured.clamp(2, 32)
}

/// `ChunkMap.getPlayerViewDistance`: the distance the client asked for, within what the
/// server allows. A client that has not said anything gets the server's.
pub fn effective_view(requested: Option<i32>, server: i32) -> i32 {
    let server = server_view(server);
    requested.map_or(server, |asked| asked.clamp(2, server))
}

/// `PlayerTicketTracker.haveTicketFor`: whether a chunk `dx`, `dz` away from the player's chunk
/// holds a `PLAYER_LOADING` ticket, which is also whether the client is sent it.
pub fn in_view(dx: i32, dz: i32, view: i32) -> bool {
    dx.abs().max(dz.abs()) <= view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_match_the_chunk_level_class() {
        assert!(is_loaded(33) && !is_loaded(34));
        assert!(is_block_ticking(32) && !is_block_ticking(33));
        assert!(is_entity_ticking(31) && !is_entity_ticking(32));
    }

    #[test]
    fn a_players_simulation_ticket_reaches_the_simulation_distance() {
        // Level 31 - 10 = 21 on the player's chunk: entity ticking out to 10 chunks, block
        // ticking one further, loaded two further still.
        let ticket = simulation_ticket_level(10);
        assert_eq!(ticket, 21);
        assert!(is_entity_ticking(level_at(ticket, 10)));
        assert!(!is_entity_ticking(level_at(ticket, 11)));
        assert!(is_block_ticking(level_at(ticket, 11)));
        assert!(is_loaded(level_at(ticket, 12)));
        assert!(!is_loaded(level_at(ticket, 13)));
        assert_eq!(simulation_ticket_level(40), 0, "never below zero");
    }

    #[test]
    fn the_client_view_is_clamped_to_the_server_like_vanilla() {
        assert_eq!(server_view(0), 2);
        assert_eq!(server_view(100), 32);
        assert_eq!(effective_view(None, 10), 10);
        assert_eq!(
            effective_view(Some(16), 10),
            10,
            "never more than the server"
        );
        assert_eq!(effective_view(Some(6), 10), 6);
        assert_eq!(effective_view(Some(0), 10), 2, "never less than two");
    }

    #[test]
    fn the_view_square_edge_is_where_loading_stops() {
        assert!(in_view(4, -4, 4));
        assert!(!in_view(5, 0, 4));
        assert!(!in_view(0, -5, 4));
    }
}
