//! `/ride`: which entity sits on which. Riders follow their vehicle every tick (the world loop
//! teleports them) and the client is told through `set_passengers`, so it plays the riding
//! pose. Vehicle steering and per-entity seat offsets are not simulated: the rider is placed
//! at the vehicle's position plus a fixed lift. A rider that sneaks gets off.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard, OnceLock};

use ferrite_protocol::{Encode, Packet, VarInt, packets::play::SetPassengers};
use uuid::Uuid;

use super::commands::{GameState, OnlinePlayer};

/// How far above the vehicle's position a rider is placed.
const SEAT_LIFT: f64 = 0.6;

static RIDES: Mutex<BTreeMap<Uuid, Uuid>> = Mutex::new(BTreeMap::new());
static DISMOUNTS: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

fn rides() -> MutexGuard<'static, BTreeMap<Uuid, Uuid>> {
    RIDES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a mount was refused (vanilla `commands.ride.*` keys).
#[derive(Debug, PartialEq, Eq)]
pub enum MountError {
    AlreadyRiding,
    CannotRidePlayer,
    Loop,
    SameEntity,
}

/// Seats `rider` on `vehicle`.
pub fn mount(rider: Uuid, vehicle: Uuid, vehicle_is_player: bool) -> Result<(), MountError> {
    if rider == vehicle {
        return Err(MountError::SameEntity);
    }
    if vehicle_is_player {
        return Err(MountError::CannotRidePlayer);
    }
    let mut rides = rides();
    if rides.contains_key(&rider) {
        return Err(MountError::AlreadyRiding);
    }
    // The vehicle may not (indirectly) ride the rider.
    let mut cursor = vehicle;
    while let Some(next) = rides.get(&cursor) {
        if *next == rider {
            return Err(MountError::Loop);
        }
        cursor = *next;
    }
    rides.insert(rider, vehicle);
    Ok(())
}

/// Takes `rider` off whatever it rides; the vehicle it left.
pub fn dismount(rider: Uuid) -> Option<Uuid> {
    rides().remove(&rider)
}

pub fn vehicle_of(rider: Uuid) -> Option<Uuid> {
    rides().get(&rider).copied()
}

pub fn passengers_of(vehicle: Uuid) -> Vec<Uuid> {
    rides()
        .iter()
        .filter(|(_, v)| **v == vehicle)
        .map(|(r, _)| *r)
        .collect()
}

/// A player pressed sneak: dismount them on the next tick.
pub fn request_dismount(rider: Uuid) {
    if rides().contains_key(&rider) {
        DISMOUNTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(rider);
    }
}

/// Forgets a player who left.
pub fn forget(uuid: Uuid) {
    rides().retain(|rider, vehicle| *rider != uuid && *vehicle != uuid);
    follows().retain(|viewer, target| *viewer != uuid && *target != uuid);
}

fn follows() -> std::sync::MutexGuard<'static, HashMap<Uuid, Uuid>> {
    static FOLLOWS: OnceLock<Mutex<HashMap<Uuid, Uuid>>> = OnceLock::new();
    FOLLOWS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// `/spectate`: the viewer's body follows the target while the camera is attached.
pub fn follow(viewer: Uuid, target: Uuid) {
    follows().insert(viewer, target);
}

pub fn unfollow(viewer: Uuid) -> bool {
    follows().remove(&viewer).is_some()
}

/// What the world loop must do after [`plan`].
#[derive(Debug, PartialEq)]
pub enum Step {
    Move {
        rider: Uuid,
        rider_is_player: bool,
        rider_pos: [f64; 3],
        rider_rot: (f32, f32),
        to: [f64; 3],
    },
    /// The rider got off (or the vehicle vanished): resend the vehicle's passenger list.
    Detach { vehicle: Uuid },
}

/// Network entity id of a player or region entity.
pub fn network_id(
    state: &GameState,
    online: &HashMap<Uuid, OnlinePlayer>,
    uuid: Uuid,
) -> Option<i32> {
    online
        .get(&uuid)
        .map(|p| p.entity)
        .or_else(|| state.entities.lock().ok()?.get(uuid).map(|e| e.network_id))
}

fn position(state: &GameState, uuid: Uuid) -> Option<([f64; 3], bool, (f32, f32))> {
    if let Some(player) = state.players.iter().find(|p| p.uuid == uuid) {
        return Some((player.pos, true, (player.yaw, player.pitch)));
    }
    state
        .entities
        .lock()
        .ok()?
        .get(uuid)
        .map(|e| (e.pos, false, (e.yaw, e.pitch)))
}

/// One tick of bookkeeping: sneaks and vanished vehicles end rides, everything else follows.
pub fn plan(state: &GameState) -> Vec<Step> {
    let mut steps = Vec::new();
    let requested: Vec<Uuid> =
        std::mem::take(&mut *DISMOUNTS.lock().unwrap_or_else(|e| e.into_inner()));
    for rider in requested {
        unfollow(rider);
        if let Some(vehicle) = dismount(rider) {
            steps.push(Step::Detach { vehicle });
        }
    }
    let followers: Vec<(Uuid, Uuid)> = follows().iter().map(|(v, t)| (*v, *t)).collect();
    for (viewer, target) in followers {
        let (Some((viewer_pos, _, rot)), Some((target_pos, _, _))) =
            (position(state, viewer), position(state, target))
        else {
            follows().remove(&viewer);
            continue;
        };
        if (0..3).any(|i| (viewer_pos[i] - target_pos[i]).abs() > 0.01) {
            steps.push(Step::Move {
                rider: viewer,
                rider_is_player: true,
                rider_pos: viewer_pos,
                rider_rot: rot,
                to: target_pos,
            });
        }
    }
    let snapshot: Vec<(Uuid, Uuid)> = rides().iter().map(|(r, v)| (*r, *v)).collect();
    for (rider, vehicle) in snapshot {
        let Some((rider_pos, rider_is_player, rider_rot)) = position(state, rider) else {
            rides().remove(&rider);
            steps.push(Step::Detach { vehicle });
            continue;
        };
        let Some((vehicle_pos, _, _)) = position(state, vehicle) else {
            rides().remove(&rider);
            steps.push(Step::Detach { vehicle });
            continue;
        };
        let to = [vehicle_pos[0], vehicle_pos[1] + SEAT_LIFT, vehicle_pos[2]];
        let moved = (0..3).any(|i| (rider_pos[i] - to[i]).abs() > 0.01);
        if moved {
            steps.push(Step::Move {
                rider,
                rider_is_player,
                rider_pos,
                rider_rot,
                to,
            });
        }
    }
    steps
}

/// The `set_passengers` packet body (with packet id) for a vehicle's current riders.
pub fn passengers_packet(
    state: &GameState,
    online: &HashMap<Uuid, OnlinePlayer>,
    vehicle: Uuid,
) -> Option<Vec<u8>> {
    let vehicle_id = network_id(state, online, vehicle)?;
    let passengers = passengers_of(vehicle)
        .into_iter()
        .filter_map(|r| network_id(state, online, r))
        .map(VarInt)
        .collect();
    let mut bytes = Vec::new();
    VarInt(SetPassengers::ID).encode(&mut bytes);
    SetPassengers {
        vehicle: VarInt(vehicle_id),
        passengers,
    }
    .encode(&mut bytes);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn spectator_follow_ends_on_unfollow_and_when_either_side_leaves() {
        let (viewer, target) = (Uuid::from_u128(901), Uuid::from_u128(902));
        follow(viewer, target);
        assert!(unfollow(viewer));
        assert!(!unfollow(viewer));
        follow(viewer, target);
        forget(target);
        assert!(!unfollow(viewer));
    }

    use super::*;

    /// The ride table is global, so the tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn mount_rules_match_the_command_errors() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (a, b, c) = (
            Uuid::from_u128(901),
            Uuid::from_u128(902),
            Uuid::from_u128(903),
        );
        assert_eq!(mount(a, a, false), Err(MountError::SameEntity));
        assert_eq!(mount(a, b, true), Err(MountError::CannotRidePlayer));
        assert_eq!(mount(a, b, false), Ok(()));
        assert_eq!(mount(a, c, false), Err(MountError::AlreadyRiding));
        assert_eq!(mount(b, a, false), Err(MountError::Loop));
        assert_eq!(vehicle_of(a), Some(b));
        assert_eq!(passengers_of(b), vec![a]);
        assert_eq!(dismount(a), Some(b));
        assert_eq!(dismount(a), None);
    }

    #[test]
    fn sneaking_ends_the_ride_on_the_next_plan() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (a, b) = (Uuid::from_u128(911), Uuid::from_u128(912));
        mount(a, b, false).unwrap();
        request_dismount(a);
        let state = GameState::default();
        let steps = plan(&state);
        assert!(steps.contains(&Step::Detach { vehicle: b }));
        assert_eq!(vehicle_of(a), None);
    }
}
