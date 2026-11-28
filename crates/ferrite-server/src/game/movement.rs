//! Vanilla/Spigot "moved too quickly" player movement check.
//!
//! Vanilla `ServerGamePacketListenerImpl.handleMovePlayer` compares the squared displacement of
//! one movement packet against a speed limit (100 while walking, 300 while fall-flying) and
//! teleports the player back when the move exceeds it. Spigot multiplies that limit by
//! `settings.moved-too-quickly-multiplier` (default 10) before comparing, which is why a stock
//! server only catches blatant cheating. The 1.21.2 `player_movement_check` and
//! `elytra_movement_check` game rules turn the check off for walking and gliding respectively.
//!
//! Server teleports, creative/spectator flight and vehicle movement are never checked: the
//! server owns the position in those cases, and re-sending it would fight the client.

/// Spigot's walking limit (`f2 = 100.0F`). It is compared against the squared displacement of
/// one movement packet, not a per-tick speed.
pub const PLAYER_MOVE_LIMIT: f64 = 100.0;
/// Spigot's fall-flying (elytra) limit (`f2 = 300.0F`).
pub const ELYTRA_MOVE_LIMIT: f64 = 300.0;
/// The slack vanilla adds before rejecting a move (`0.0625`).
pub const MOVE_TOLERANCE: f64 = 0.0625;
/// `spigot.yml` `settings.moved-too-quickly-multiplier` default.
pub const DEFAULT_MULTIPLIER: f64 = 10.0;
/// `spigot.yml` `settings.moved-wrongly-threshold` default: the slack added to the squared
/// movement limit before a packet is rejected.
pub const DEFAULT_MOVED_WRONGLY_THRESHOLD: f64 = MOVE_TOLERANCE;

/// The movement-check game rules for one tick (`/gamerule` changes apply immediately because the
/// region rebuilds this from [`ferrite_command::gamerule::GameRules`] every tick).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CheckConfig {
    /// `spigot.yml` `settings.moved-too-quickly-multiplier`, read at startup.
    pub multiplier: f64,
    /// `spigot.yml` `settings.moved-wrongly-threshold`: slack added to the squared limit.
    pub moved_wrongly_threshold: f64,
    /// `player_movement_check`.
    pub player_movement_check: bool,
    /// `elytra_movement_check`.
    pub elytra_movement_check: bool,
}

impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            multiplier: DEFAULT_MULTIPLIER,
            moved_wrongly_threshold: DEFAULT_MOVED_WRONGLY_THRESHOLD,
            player_movement_check: true,
            elytra_movement_check: true,
        }
    }
}

/// The player state that exempts a move from the speed check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveContext {
    /// A server teleport is awaiting the client's acknowledgement.
    pub teleport_pending: bool,
    /// `PlayerAbilities.flying` (creative flight).
    pub flying: bool,
    /// The player rides a vehicle; `MoveVehicle` owns that position.
    pub riding: bool,
    /// Fall-flying (elytra) uses the larger limit.
    pub gliding: bool,
    /// Creative or spectator movement is never checked.
    pub creative_or_spectator: bool,
}

/// Whether one movement packet must be rejected ("moved too quickly!").
///
/// Mirrors vanilla's order: exempt states first, then the game rule for the movement mode, then
/// the squared-displacement limit scaled by the Spigot multiplier. The multiplier is clamped to
/// at least one so a malformed `0` cannot reject every move.
pub fn moved_too_quickly(
    from: [f64; 3],
    to: [f64; 3],
    context: MoveContext,
    config: CheckConfig,
) -> bool {
    if context.teleport_pending || context.flying || context.riding || context.creative_or_spectator
    {
        return false;
    }
    let enabled = if context.gliding {
        config.elytra_movement_check
    } else {
        config.player_movement_check
    };
    if !enabled {
        return false;
    }
    let base = if context.gliding {
        ELYTRA_MOVE_LIMIT
    } else {
        PLAYER_MOVE_LIMIT
    };
    let limit = base * config.multiplier.max(1.0);
    squared_distance(from, to) > limit + config.moved_wrongly_threshold.max(0.0)
}

fn squared_distance(from: [f64; 3], to: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walking() -> MoveContext {
        MoveContext {
            teleport_pending: false,
            flying: false,
            riding: false,
            gliding: false,
            creative_or_spectator: false,
        }
    }

    fn gliding() -> MoveContext {
        MoveContext {
            gliding: true,
            ..walking()
        }
    }

    #[test]
    fn sprint_jumps_and_normal_walking_never_trip_the_check() {
        let config = CheckConfig::default();
        // One packet of a sprint-jump: about 0.4 blocks forward and 0.42 up.
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [0.9, 64.42, 0.5],
            walking(),
            config
        ));
        // A laggy client sending four ticks of sprinting in one packet.
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [2.9, 64.0, 0.5],
            walking(),
            config
        ));
    }

    #[test]
    fn a_blink_past_the_threshold_is_rejected() {
        let config = CheckConfig::default();
        // The default multiplier is 10, so walking allows sqrt(1000) ~= 31.6 blocks per packet.
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [30.0, 64.0, 0.5],
            walking(),
            config
        ));
        assert!(moved_too_quickly(
            [0.5, 64.0, 0.5],
            [40.0, 64.0, 0.5],
            walking(),
            config
        ));
        // Spigot's threshold before the multiplier: 10 blocks per packet.
        let strict = CheckConfig {
            multiplier: 1.0,
            ..config
        };
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [10.4, 64.0, 0.5],
            walking(),
            strict
        ));
        assert!(moved_too_quickly(
            [0.5, 64.0, 0.5],
            [11.0, 64.0, 0.5],
            walking(),
            strict
        ));
        // The multiplier scales the limit: 20 blocks is fine at 10x, a cheat at 1x.
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [20.5, 64.0, 0.5],
            walking(),
            config
        ));
        assert!(moved_too_quickly(
            [0.5, 64.0, 0.5],
            [20.5, 64.0, 0.5],
            walking(),
            strict
        ));
        // The configured moved-wrongly threshold is the slack on the squared limit.
        let slack = CheckConfig {
            multiplier: 1.0,
            moved_wrongly_threshold: 4.0,
            ..config
        };
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [10.5, 64.0, 0.5],
            walking(),
            slack
        ));
    }

    #[test]
    fn elytra_flight_uses_the_larger_limit_and_its_own_rule() {
        let config = CheckConfig::default();
        // A normal glide: ~1.5 blocks in one packet.
        assert!(!moved_too_quickly(
            [0.5, 100.0, 0.5],
            [2.0, 100.1, 0.5],
            gliding(),
            config
        ));
        // 50 blocks is still inside the 300 * 10 elytra limit, 60 is not.
        assert!(!moved_too_quickly(
            [0.5, 100.0, 0.5],
            [50.5, 100.0, 0.5],
            gliding(),
            config
        ));
        assert!(moved_too_quickly(
            [0.5, 100.0, 0.5],
            [60.5, 100.0, 0.5],
            gliding(),
            config
        ));
        // `elytra_movement_check false` exempts gliding without touching walking.
        let no_elytra = CheckConfig {
            elytra_movement_check: false,
            ..config
        };
        assert!(!moved_too_quickly(
            [0.5, 100.0, 0.5],
            [60.5, 100.0, 0.5],
            gliding(),
            no_elytra
        ));
        assert!(moved_too_quickly(
            [0.5, 64.0, 0.5],
            [40.0, 64.0, 0.5],
            walking(),
            no_elytra
        ));
        // `player_movement_check false` exempts walking without touching elytra.
        let no_player = CheckConfig {
            player_movement_check: false,
            ..config
        };
        assert!(!moved_too_quickly(
            [0.5, 64.0, 0.5],
            [40.0, 64.0, 0.5],
            walking(),
            no_player
        ));
        assert!(moved_too_quickly(
            [0.5, 100.0, 0.5],
            [60.5, 100.0, 0.5],
            gliding(),
            no_player
        ));
    }

    #[test]
    fn teleports_and_server_owned_positions_are_exempt() {
        let config = CheckConfig::default();
        let far = [500.5, 64.0, 500.5];
        let from = [0.5, 64.0, 0.5];
        for context in [
            MoveContext {
                teleport_pending: true,
                ..walking()
            },
            MoveContext {
                flying: true,
                ..walking()
            },
            MoveContext {
                riding: true,
                ..walking()
            },
            MoveContext {
                creative_or_spectator: true,
                ..walking()
            },
        ] {
            assert!(
                !moved_too_quickly(from, far, context, config),
                "{context:?} must not be checked"
            );
        }
        assert!(moved_too_quickly(from, far, walking(), config));
    }
}
