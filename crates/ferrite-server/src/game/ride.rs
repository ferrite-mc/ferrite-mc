//! `/ride`: which entity sits on which. Riders follow their vehicle every tick (the world loop
//! teleports them) and the client is told through `set_passengers`, so it plays the riding
//! pose. Vehicle steering and per-entity seat offsets are not simulated: the rider is placed
//! at the vehicle's position plus a fixed lift. A rider that sneaks gets off.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use ferrite_entity::shoulder::{ShoulderParrot, ShoulderSide, Shoulders};
use ferrite_protocol::{Encode, Packet, VarInt, packets::play::SetPassengers};
use uuid::Uuid;

use super::commands::{GameState, OnlinePlayer};

/// How far above the vehicle's position a rider is placed.
const SEAT_LIFT: f64 = 0.6;

static RIDES: Mutex<BTreeMap<Uuid, Uuid>> = Mutex::new(BTreeMap::new());
/// Shoulder parrots by player UUID. A shoulder is a passenger of the player, but it has no world
/// entity while it sits there, so this small per-player table is the region-owned representation.
/// It is bounded by the online player count, only touched on pickup/release and player ticks,
/// and moves with the player's UUID across region and dimension hand-offs.
static SHOULDERS: Mutex<BTreeMap<Uuid, Shoulders>> = Mutex::new(BTreeMap::new());
/// Players whose shoulder save was already read. A reconnect re-reads it because `forget`
/// clears this set.
static SHOULDER_LOADED: Mutex<Option<HashSet<Uuid>>> = Mutex::new(None);
/// Vehicles whose rider steers them from the client (a saddled horse): the vehicle follows the
/// rider packets instead of the rider following the vehicle.
static CONTROLLED: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

/// Marks or unmarks `vehicle` as steered by its rider.
pub fn set_controlled(vehicle: Uuid, controlled: bool) {
    let mut all = CONTROLLED.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|v| *v != vehicle);
    if controlled {
        all.push(vehicle);
    }
}

fn is_controlled(vehicle: Uuid) -> bool {
    CONTROLLED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&vehicle)
}

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
    forget_shoulders(uuid);
}

fn shoulders_map() -> MutexGuard<'static, BTreeMap<Uuid, Shoulders>> {
    SHOULDERS.lock().unwrap_or_else(|e| e.into_inner())
}

fn shoulder_loaded() -> MutexGuard<'static, Option<HashSet<Uuid>>> {
    SHOULDER_LOADED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Claims the first shoulder load of a player this session. Returns `true` exactly once per
/// login, so the region can read the save file and announce the parrots to viewers.
pub fn ensure_shoulder_entry(player: Uuid) -> bool {
    let mut loaded = shoulder_loaded();
    let loaded = loaded.get_or_insert_with(HashSet::new);
    if !loaded.insert(player) {
        return false;
    }
    shoulders_map().entry(player).or_default();
    true
}

/// Installs the shoulders read from the player's save file.
pub fn restore_shoulders(player: Uuid, shoulders: Shoulders) {
    shoulders_map().insert(player, shoulders);
}

pub fn shoulders_of(player: Uuid) -> Shoulders {
    shoulders_map().get(&player).copied().unwrap_or_default()
}

/// Puts a parrot on the first free shoulder; `None` when both are occupied.
pub fn shoulder_put(player: Uuid, parrot: ShoulderParrot) -> Option<ShoulderSide> {
    shoulders_map().entry(player).or_default().put(parrot)
}

/// Puts `parrot` on `side` (or empties it), returning the parrot that was there.
pub fn shoulder_set(
    player: Uuid,
    side: ShoulderSide,
    parrot: Option<ShoulderParrot>,
) -> Option<ShoulderParrot> {
    let mut all = shoulders_map();
    let shoulders = all.entry(player).or_default();
    let old = shoulders.get(side);
    shoulders.set(side, parrot);
    old
}

/// Takes the parrot off one shoulder.
pub fn shoulder_release(player: Uuid, side: ShoulderSide) -> Option<ShoulderParrot> {
    let mut all = shoulders_map();
    all.get_mut(&player)?.release(side)
}

/// Drops the in-memory shoulders of a player who left. The save file is written on every
/// pickup/release, so there is nothing to flush here.
pub fn forget_shoulders(player: Uuid) {
    shoulders_map().remove(&player);
    if let Some(loaded) = shoulder_loaded().as_mut() {
        loaded.remove(&player);
    }
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

/// Serializes tests that use the process-global ride table with the world loop's [`plan`],
/// which forgets rides whose entities are not in its state. A concurrently running test
/// world would otherwise clear another test's rides mid-sequence.
#[cfg(test)]
static TEST_TURN: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_turn() -> MutexGuard<'static, ()> {
    TEST_TURN.lock().unwrap_or_else(|e| e.into_inner())
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
        if is_controlled(vehicle) {
            continue;
        }
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
        let _turn = test_turn();
        let (viewer, target) = (Uuid::from_u128(901), Uuid::from_u128(902));
        follow(viewer, target);
        assert!(unfollow(viewer));
        assert!(!unfollow(viewer));
        follow(viewer, target);
        forget(target);
        assert!(!unfollow(viewer));
    }

    use super::*;

    /// The ride table is global, so the tests take turns with the world loop's plans.
    #[test]
    fn mount_rules_match_the_command_errors() {
        let _turn = test_turn();
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
        let _turn = test_turn();
        let (a, b) = (Uuid::from_u128(911), Uuid::from_u128(912));
        mount(a, b, false).unwrap();
        request_dismount(a);
        let state = GameState::default();
        let steps = plan(&state);
        assert!(steps.contains(&Step::Detach { vehicle: b }));
        assert_eq!(vehicle_of(a), None);
    }

    /// A `GameState` with non-player entities at known positions, like the command snapshot
    /// the world loop publishes for selectors and ride planning.
    fn state_with(entries: &[(Uuid, i32, [f64; 3])]) -> GameState {
        let state = GameState::default();
        {
            let mut index = state.entities.lock().unwrap();
            for &(uuid, network_id, pos) in entries {
                index.insert(super::super::entity_index::CommandEntity {
                    uuid,
                    network_id,
                    kind: 0,
                    pos,
                    yaw: 0.0,
                    pitch: 0.0,
                    dimension: super::super::Dimension::Overworld,
                    tags: Default::default(),
                    custom_name: None,
                });
            }
        }
        state
    }

    #[test]
    fn a_teleported_vehicle_carries_its_rider_without_orphaning_it() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(921), Uuid::from_u128(922));
        mount(rider, vehicle, false).unwrap();
        let state = state_with(&[
            (rider, 1, [0.5, 64.0 + SEAT_LIFT, 0.5]),
            (vehicle, 2, [0.5, 64.0, 0.5]),
        ]);
        assert!(plan(&state).is_empty(), "already in the seat");

        // `/tp` or a region hand-off moves the vehicle; the rider follows on the next tick.
        state
            .entities
            .lock()
            .unwrap()
            .insert(super::super::entity_index::CommandEntity {
                uuid: vehicle,
                network_id: 2,
                kind: 0,
                pos: [300.5, 70.0, -40.5],
                yaw: 0.0,
                pitch: 0.0,
                dimension: super::super::Dimension::Overworld,
                tags: Default::default(),
                custom_name: None,
            });
        let steps = plan(&state);
        assert_eq!(
            steps,
            [Step::Move {
                rider,
                rider_is_player: false,
                rider_pos: [0.5, 64.0 + SEAT_LIFT, 0.5],
                rider_rot: (0.0, 0.0),
                to: [300.5, 70.0 + SEAT_LIFT, -40.5],
            }]
        );
        assert_eq!(vehicle_of(rider), Some(vehicle));
        forget(rider);
        forget(vehicle);
    }

    #[test]
    fn knockback_does_not_leave_a_rider_off_its_vehicle() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(931), Uuid::from_u128(932));
        mount(rider, vehicle, false).unwrap();
        // An explosion or a piston shoves the rider two blocks away; the seat is authoritative.
        let state = state_with(&[(rider, 1, [2.5, 64.0, 0.5]), (vehicle, 2, [0.5, 64.0, 0.5])]);
        let steps = plan(&state);
        assert_eq!(
            steps,
            [Step::Move {
                rider,
                rider_is_player: false,
                rider_pos: [2.5, 64.0, 0.5],
                rider_rot: (0.0, 0.0),
                to: [0.5, 64.0 + SEAT_LIFT, 0.5],
            }]
        );
        assert_eq!(passengers_of(vehicle), vec![rider]);
        forget(rider);
        forget(vehicle);
    }

    #[test]
    fn a_vehicle_that_leaves_detaches_its_passenger_instead_of_orphaning_it() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(941), Uuid::from_u128(942));
        mount(rider, vehicle, false).unwrap();
        // The vehicle was killed or removed from the command snapshot.
        let state = state_with(&[(rider, 1, [0.5, 64.6, 0.5])]);
        assert_eq!(plan(&state), [Step::Detach { vehicle }]);
        assert_eq!(vehicle_of(rider), None);
        assert!(passengers_of(vehicle).is_empty());

        // The rider leaving removes the reverse mapping too, so no later vehicle can inherit it.
        mount(rider, vehicle, false).unwrap();
        forget(rider);
        assert_eq!(vehicle_of(rider), None);
        assert!(passengers_of(vehicle).is_empty());
        forget(vehicle);
    }

    #[test]
    fn shoulder_parrots_fill_left_then_right_and_are_forgotten_on_leave() {
        let _turn = test_turn();
        let player = Uuid::from_u128(951);
        let parrot = |n| ShoulderParrot {
            uuid: Uuid::from_u128(n),
            variant: 1,
            owner: Some(player),
        };
        assert!(ensure_shoulder_entry(player));
        assert!(!ensure_shoulder_entry(player), "only the first tick loads");
        assert_eq!(shoulder_put(player, parrot(1)), Some(ShoulderSide::Left));
        assert_eq!(shoulder_put(player, parrot(2)), Some(ShoulderSide::Right));
        assert_eq!(shoulder_put(player, parrot(3)), None);
        assert_eq!(
            shoulders_of(player)
                .get(ShoulderSide::Right)
                .unwrap()
                .variant,
            1
        );
        assert_eq!(
            shoulder_release(player, ShoulderSide::Left).unwrap().uuid,
            Uuid::from_u128(1)
        );
        assert!(shoulders_of(player).left.is_none());
        assert!(shoulders_of(player).right.is_some());
        forget(player);
        assert!(shoulders_of(player).is_empty());
        assert!(ensure_shoulder_entry(player), "a reconnect loads again");
        forget_shoulders(player);
    }
}
