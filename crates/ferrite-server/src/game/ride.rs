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

/// Where the rider goes when the generated entity data has no attachment points at all (an old
/// extraction): a fixed lift above the vehicle, the placement used before attachments.
const FALLBACK_LIFT: f64 = 0.6;

/// One named single-point attachment (`passenger`, `vehicle`, ...) of an entity type from the
/// generated vanilla data, `[0, 0, 0]` when the type has none.
fn attachment(kind: &str, name: &str) -> Option<[f64; 3]> {
    let props = ferrite_data::entity_props::by_name(kind)?;
    props
        .attachments
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
}

/// Where a rider of type `rider` sits relative to the origin of a `vehicle` facing `yaw`.
///
/// Vanilla `Entity.positionRider`: the vehicle's `PASSENGER` attachment point, turned by the
/// vehicle's yaw (`EntityAttachments.transformPoint`: `yRot(-yaw * pi/180)`), minus the rider's own
/// `VEHICLE` attachment point. Scale (babies) and the horse rearing offset
/// (`AbstractHorse.getPassengerAttachmentPoint` with `standAnim` 0) are not applied.
pub fn seat_offset(vehicle: &str, yaw: f32, rider: &str) -> [f64; 3] {
    let (Some(seat), Some(hold)) = (
        attachment(vehicle, "passenger"),
        attachment(rider, "vehicle"),
    ) else {
        return [0.0, FALLBACK_LIFT, 0.0];
    };
    let angle = -f64::from(yaw) * f64::from(0.017_453_292_f32);
    let (sin, cos) = angle.sin_cos();
    [
        seat[0] * cos + seat[2] * sin - hold[0],
        seat[1] - hold[1],
        seat[2] * cos - seat[0] * sin - hold[2],
    ]
}

/// One seated rider: the vehicle and the order the rider got on in (vanilla keeps passengers in
/// the order they were added, and the first one steers).
#[derive(Clone, Copy)]
struct Seat {
    vehicle: Uuid,
    order: u64,
}

static RIDES: Mutex<BTreeMap<Uuid, Seat>> = Mutex::new(BTreeMap::new());
static NEXT_ORDER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
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

fn rides() -> MutexGuard<'static, BTreeMap<Uuid, Seat>> {
    RIDES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a mount was refused (vanilla `commands.ride.*` keys).
#[derive(Debug, PartialEq, Eq)]
pub enum MountError {
    AlreadyRiding,
    CannotRidePlayer,
    Loop,
    SameEntity,
    /// Every seat of the vehicle is taken.
    Full,
}

/// How many riders a vehicle of `kind` takes, read from the 26.3 `canAddPassenger` overrides:
/// the default needs an empty vehicle, `AbstractBoat` allows `getMaxPassengers()` = 2 (chest
/// boats 1), `Camel` allows `size() <= 2` (so three) and `HappyGhast` `size() < 4`.
pub fn capacity_of(kind: &str) -> usize {
    let name = kind.strip_prefix("minecraft:").unwrap_or(kind);
    if name.ends_with("_chest_boat") || name.ends_with("_chest_raft") {
        1
    } else if name.ends_with("_boat") || name.ends_with("_raft") {
        2
    } else if name == "camel" {
        3
    } else if name == "happy_ghast" {
        4
    } else {
        1
    }
}

/// Seats `rider` on `vehicle`.
pub fn mount(rider: Uuid, vehicle: Uuid, vehicle_is_player: bool) -> Result<(), MountError> {
    mount_in(rider, vehicle, vehicle_is_player, None)
}

/// [`mount`] for a vehicle whose entity type is known, so its seat count applies. An unknown
/// type gets the default single seat.
pub fn mount_in(
    rider: Uuid,
    vehicle: Uuid,
    vehicle_is_player: bool,
    vehicle_kind: Option<&str>,
) -> Result<(), MountError> {
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
        if next.vehicle == rider {
            return Err(MountError::Loop);
        }
        cursor = next.vehicle;
    }
    let seated = rides.values().filter(|s| s.vehicle == vehicle).count();
    if seated >= vehicle_kind.map_or(1, capacity_of) {
        return Err(MountError::Full);
    }
    let order = NEXT_ORDER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    rides.insert(rider, Seat { vehicle, order });
    Ok(())
}

/// Takes `rider` off whatever it rides; the vehicle it left.
pub fn dismount(rider: Uuid) -> Option<Uuid> {
    rides().remove(&rider).map(|s| s.vehicle)
}

pub fn vehicle_of(rider: Uuid) -> Option<Uuid> {
    rides().get(&rider).map(|s| s.vehicle)
}

pub fn passengers_of(vehicle: Uuid) -> Vec<Uuid> {
    let rides = rides();
    let mut seated: Vec<(u64, Uuid)> = rides
        .iter()
        .filter(|(_, s)| s.vehicle == vehicle)
        .map(|(r, s)| (s.order, *r))
        .collect();
    seated.sort_unstable();
    seated.into_iter().map(|(_, r)| r).collect()
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
    rides().retain(|rider, seat| *rider != uuid && seat.vehicle != uuid);
    follows().retain(|viewer, target| *viewer != uuid && *target != uuid);
    forget_shoulders(uuid);
    super::targeting::forget(uuid);
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
        /// The dimension both rider and vehicle are in.
        dimension: super::Dimension,
        to: [f64; 3],
    },
    /// The rider got off (or the vehicle vanished): resend the vehicle's passenger list.
    Detach { vehicle: Uuid },
}

/// The entity type of `uuid` (`minecraft:oak_boat`), for seat counts; `None` for a player or an
/// entity the command index does not know.
pub fn kind_of(state: &GameState, uuid: Uuid) -> Option<&'static str> {
    state.entities.lock().ok()?.get(uuid).map(|e| e.type_name())
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

/// The entity type id (`minecraft:horse`, `minecraft:player`) of a published rider or vehicle.
fn kind_key(state: &GameState, uuid: Uuid) -> Option<&'static str> {
    if state.players.iter().any(|p| p.uuid == uuid) {
        return Some("minecraft:player");
    }
    state.entities.lock().ok()?.get(uuid).map(|e| e.type_name())
}

/// The dimension a player or region entity is published in.
fn dimension_of(state: &GameState, uuid: Uuid) -> Option<super::Dimension> {
    if let Some(player) = state.players.iter().find(|p| p.uuid == uuid) {
        return Some(player.dimension);
    }
    state.entities.lock().ok()?.get(uuid).map(|e| e.dimension)
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
                dimension: dimension_of(state, viewer).unwrap_or(super::Dimension::Overworld),
                to: target_pos,
            });
        }
    }
    let mut snapshot: Vec<(Uuid, Uuid)> = rides().iter().map(|(r, s)| (*r, s.vehicle)).collect();
    // Riders of riders must be placed after the rider under them, on that rider's new seat, so
    // process shallow chains first and remember where each rider ends up this tick.
    let depth_of = |start: Uuid| {
        let by_rider: HashMap<Uuid, Uuid> = snapshot.iter().copied().collect();
        let (mut depth, mut cursor) = (0usize, start);
        while let Some(next) = by_rider.get(&cursor) {
            depth += 1;
            cursor = *next;
            if depth > snapshot.len() {
                break;
            }
        }
        depth
    };
    let mut order: Vec<(usize, Uuid, Uuid)> = snapshot
        .iter()
        .map(|(r, v)| (depth_of(*v), *r, *v))
        .collect();
    order.sort_unstable();
    snapshot = order.into_iter().map(|(_, r, v)| (r, v)).collect();
    let mut placed: HashMap<Uuid, [f64; 3]> = HashMap::new();
    for (rider, vehicle) in snapshot {
        let Some((rider_pos, rider_is_player, rider_rot)) = position(state, rider) else {
            rides().remove(&rider);
            steps.push(Step::Detach { vehicle });
            continue;
        };
        let Some((live_vehicle_pos, _, vehicle_rot)) = position(state, vehicle) else {
            rides().remove(&rider);
            steps.push(Step::Detach { vehicle });
            continue;
        };
        // Vanilla ends a ride when either side changes dimension; coordinates of another world
        // are meaningless here.
        if dimension_of(state, rider) != dimension_of(state, vehicle) {
            rides().remove(&rider);
            steps.push(Step::Detach { vehicle });
            continue;
        }
        if is_controlled(vehicle) {
            continue;
        }
        let vehicle_pos = placed.get(&vehicle).copied().unwrap_or(live_vehicle_pos);
        let seat = seat_offset(
            kind_key(state, vehicle).unwrap_or(""),
            vehicle_rot.0,
            kind_key(state, rider).unwrap_or(""),
        );
        let to = [
            vehicle_pos[0] + seat[0],
            vehicle_pos[1] + seat[1],
            vehicle_pos[2] + seat[2],
        ];
        placed.insert(rider, to);
        let moved = (0..3).any(|i| (rider_pos[i] - to[i]).abs() > 0.01);
        if moved {
            steps.push(Step::Move {
                rider,
                rider_is_player,
                rider_pos,
                rider_rot,
                dimension: dimension_of(state, rider).unwrap_or(super::Dimension::Overworld),
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

    fn pig_kind() -> u16 {
        ferrite_data::entity_props::by_name("minecraft:pig")
            .expect("the vanilla data knows the pig")
            .id
    }

    /// Pig on pig: the pig's PASSENGER point (0.86875 in the 26.3 data) minus the zero
    /// VEHICLE point of the rider, which is what the test entities are.
    fn lift() -> f64 {
        f64::from(0.86875_f32)
    }

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
    fn passengers_keep_the_order_they_mounted_in() {
        let _turn = test_turn();
        // UUID order is the reverse of mount order, so a UUID-sorted list would fail this.
        let (first, second, third) = (
            Uuid::from_u128(953),
            Uuid::from_u128(952),
            Uuid::from_u128(951),
        );
        let boat = Uuid::from_u128(950);
        mount_in(first, boat, false, Some("minecraft:oak_boat")).unwrap();
        mount_in(second, boat, false, Some("minecraft:oak_boat")).unwrap();
        assert_eq!(passengers_of(boat), vec![first, second]);
        dismount(first);
        mount_in(third, boat, false, Some("minecraft:oak_boat")).unwrap();
        assert_eq!(
            passengers_of(boat),
            vec![second, third],
            "a new rider sits last"
        );
        dismount(second);
        dismount(third);
    }

    #[test]
    fn a_vehicle_takes_only_as_many_riders_as_vanilla_allows() {
        let _turn = test_turn();
        // Read from 26.3: `Entity.canAddPassenger` needs an empty vehicle, `AbstractBoat` allows
        // `getMaxPassengers()` = 2 (1 for chest boats), `Camel` allows `size() <= 2` and
        // `HappyGhast` `size() < 4`.
        for (kind, seats) in [
            ("minecraft:horse", 1),
            ("minecraft:pig", 1),
            ("minecraft:oak_boat", 2),
            ("minecraft:bamboo_raft", 2),
            ("minecraft:oak_chest_boat", 1),
            ("minecraft:camel", 3),
            ("minecraft:happy_ghast", 4),
        ] {
            assert_eq!(capacity_of(kind), seats, "{kind}");
            let vehicle = Uuid::from_u128(960);
            let riders: Vec<Uuid> = (0..6).map(|i| Uuid::from_u128(961 + i)).collect();
            let mut seated = 0;
            for rider in &riders {
                match mount_in(*rider, vehicle, false, Some(kind)) {
                    Ok(()) => seated += 1,
                    Err(error) => assert_eq!(error, MountError::Full, "{kind}"),
                }
            }
            assert_eq!(seated, seats, "{kind}");
            for rider in &riders {
                dismount(*rider);
            }
        }
    }

    #[test]
    fn mounting_without_a_known_type_uses_the_default_single_seat() {
        let _turn = test_turn();
        let (a, b, vehicle) = (
            Uuid::from_u128(971),
            Uuid::from_u128(972),
            Uuid::from_u128(970),
        );
        assert_eq!(mount(a, vehicle, false), Ok(()));
        assert_eq!(mount(b, vehicle, false), Err(MountError::Full));
        dismount(a);
    }

    #[test]
    fn a_rider_of_a_rider_follows_the_final_position_in_the_same_plan() {
        let _turn = test_turn();
        let (top, middle, base) = (
            Uuid::from_u128(981),
            Uuid::from_u128(982),
            Uuid::from_u128(983),
        );
        mount(middle, base, false).unwrap();
        mount(top, middle, false).unwrap();
        // The base moved; both riders are still where the base used to be.
        let state = state_with(&[
            (top, 1, [0.5, 64.0 + lift() * 2.0, 0.5]),
            (middle, 2, [0.5, 64.0 + lift(), 0.5]),
            (base, 3, [10.5, 64.0, 0.5]),
        ]);
        let steps = plan(&state);
        let target = |who: Uuid| {
            steps.iter().find_map(|step| match step {
                Step::Move { rider, to, .. } if *rider == who => Some(*to),
                _ => None,
            })
        };
        assert_eq!(target(middle), Some([10.5, 64.0 + lift(), 0.5]));
        assert_eq!(
            target(top),
            Some([10.5, 64.0 + lift() + lift(), 0.5]),
            "the top rider sits on the middle rider's new seat, not its old one"
        );
        dismount(top);
        dismount(middle);
    }

    #[test]
    fn a_leaving_player_frees_the_seat_and_detaches_their_passengers() {
        let _turn = test_turn();
        let (driver, passenger, vehicle) = (
            Uuid::from_u128(991),
            Uuid::from_u128(992),
            Uuid::from_u128(990),
        );
        mount(driver, vehicle, false).unwrap();
        forget(driver);
        assert_eq!(vehicle_of(driver), None);
        assert_eq!(
            mount(passenger, vehicle, false),
            Ok(()),
            "the seat is free again"
        );
        forget(vehicle);
        assert_eq!(
            vehicle_of(passenger),
            None,
            "a vanished vehicle keeps no riders"
        );
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
                    kind: pig_kind(),
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
    fn seats_follow_the_vanilla_attachment_points() {
        // Values read from the 26.3 data (`PASSENGER` of the vehicle, `VEHICLE` of the rider).
        let at = |vehicle, yaw, rider| seat_offset(vehicle, yaw, rider);
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6);
        // A player (vehicle point y 0.6) on a horse (passenger y 1.44375).
        assert!(near(
            at("minecraft:horse", 0.0, "minecraft:player"),
            [0.0, 1.44375 - 0.6, 0.0]
        ));
        assert!(near(
            at("minecraft:donkey", 0.0, "minecraft:player"),
            [0.0, 1.1125 - 0.6, 0.0]
        ));
        assert!(near(
            at("minecraft:mule", 0.0, "minecraft:player"),
            [0.0, 1.2125 - 0.6, 0.0]
        ));
        // A zombie rider hangs 0.7 below its seat.
        assert!(near(
            at("minecraft:horse", 0.0, "minecraft:zombie"),
            [0.0, 1.44375 - 0.7, 0.0]
        ));
        // A llama's seat is 0.3 behind its centre and turns with its yaw: yaw 0 faces +z, so
        // behind is -z; at yaw 90 (facing -x) behind is +x.
        assert!(near(
            at("minecraft:llama", 0.0, "minecraft:player"),
            [0.0, 1.37 - 0.6, -0.3]
        ));
        assert!(near(
            at("minecraft:llama", 90.0, "minecraft:player"),
            [0.3, 1.37 - 0.6, 0.0]
        ));
        assert!(near(
            at("minecraft:trader_llama", 180.0, "minecraft:player"),
            [0.0, 1.37 - 0.6, 0.3]
        ));
    }

    #[test]
    fn changing_dimension_ends_the_ride_instead_of_moving_the_rider_across_worlds() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(1001), Uuid::from_u128(1002));
        mount(rider, vehicle, false).unwrap();
        let state = state_with(&[
            (rider, 1, [0.5, 64.0, 0.5]),
            (vehicle, 2, [100.5, 64.0, 0.5]),
        ]);
        {
            // The vehicle is published by a Nether region.
            let mut index = state.entities.lock().unwrap();
            let mut moved = index.get(vehicle).unwrap().clone();
            moved.dimension = super::super::Dimension::Nether;
            index.insert(moved);
        }
        let steps = plan(&state);
        assert_eq!(
            steps,
            vec![Step::Detach { vehicle }],
            "no Move to coordinates in another dimension"
        );
        assert_eq!(vehicle_of(rider), None, "the ride ended");
    }

    #[test]
    fn a_rider_in_the_nether_is_moved_in_the_nether() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(1011), Uuid::from_u128(1012));
        mount(rider, vehicle, false).unwrap();
        let state = state_with(&[(rider, 1, [0.5, 64.0, 0.5]), (vehicle, 2, [9.5, 64.0, 0.5])]);
        {
            let mut index = state.entities.lock().unwrap();
            for uuid in [rider, vehicle] {
                let mut moved = index.get(uuid).unwrap().clone();
                moved.dimension = super::super::Dimension::Nether;
                index.insert(moved);
            }
        }
        let steps = plan(&state);
        assert!(
            matches!(
                steps.as_slice(),
                [Step::Move {
                    dimension: super::super::Dimension::Nether,
                    ..
                }]
            ),
            "{steps:?}"
        );
        forget(rider);
        forget(vehicle);
    }

    #[test]
    fn a_teleported_vehicle_carries_its_rider_without_orphaning_it() {
        let _turn = test_turn();
        let (rider, vehicle) = (Uuid::from_u128(921), Uuid::from_u128(922));
        mount(rider, vehicle, false).unwrap();
        let state = state_with(&[
            (rider, 1, [0.5, 64.0 + lift(), 0.5]),
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
                kind: pig_kind(),
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
                rider_pos: [0.5, 64.0 + lift(), 0.5],
                rider_rot: (0.0, 0.0),
                dimension: super::super::Dimension::Overworld,
                to: [300.5, 70.0 + lift(), -40.5],
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
                dimension: super::super::Dimension::Overworld,
                to: [0.5, 64.0 + lift(), 0.5],
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
