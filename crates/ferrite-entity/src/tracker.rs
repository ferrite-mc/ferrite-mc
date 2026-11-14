//! The vanilla entity tracker (`ChunkMap.TrackedEntity` + `ServerEntity`), per region.
//!
//! Each tick [`track`] pairs every entity of a store with the players of the same region that
//! should see it (horizontal distance within the type's tracking range capped by the player's
//! view distance, and the entity's chunk sent to that player), then sends what changed:
//!
//! - start/stop tracking: `add_entity` plus the non-default data values, `remove_entities`;
//! - movement on the type's update interval (or when forced): `move_entity_pos`,
//!   `move_entity_pos_rot` and `move_entity_rot` deltas in 1/4096 block units against the last
//!   sent position, falling back to a full `entity_position_sync` when a delta overflows, the
//!   on-ground state changes, 400 updates pass, or an item entity resting against a block
//!   would lose precision; a forced position refresh every 60 ticks;
//! - `set_entity_motion` for types that track deltas, when the velocity changed;
//! - `set_entity_data` with only the dirty values; `rotate_head`; `teleport_entity`.
//!
//! Players never leave the tracker while they cross into a chunk owned by the same region, but
//! a region hand-off removes them from the region's player list for a moment, so a pairing
//! whose player is missing is kept for [`ABSENT_GRACE_TICKS`] before it is dropped.

use crate::store::{EntityStore, Flags, chunk_of};
use ferrite_math::Vec3;
use ferrite_protocol::{
    Packet, VarInt, encode_packet,
    packets::play::{
        AddEntity, AttributeSnapshot, EntityHeadRotation, EntityMove, EntityMoveRotation,
        EntityRotation, EntityTeleport, RemoveEntities, SetEntityData, SetEntityLink,
        SetEntityMotion, SetEquipment, TeleportEntity, UpdateAttributes, UpdateMobEffect,
    },
    types::LpVec3,
};
use std::collections::HashSet;
use uuid::Uuid;

/// A tracking player: UUID plus a session number, so a reconnect is a new viewer.
pub type ViewerKey = (Uuid, u64);

/// Ticks a pairing survives while its player is missing from the region.
pub const ABSENT_GRACE_TICKS: u8 = 20;
/// Vanilla `ServerEntity.TOLERANCE_LEVEL_POSITION` (a float constant widened to double).
const POSITION_TOLERANCE: f64 = 7.629_394_5E-6_f32 as f64;
const FORCED_POS_UPDATE_PERIOD: u32 = 60;
const FORCED_TELEPORT_PERIOD: u32 = 400;

/// A player as the tracker sees it.
pub struct Viewer<'a> {
    pub key: ViewerKey,
    pub pos: Vec3,
    /// View distance in chunks.
    pub view_distance: i32,
    /// Chunks already sent to the player (vanilla `isChunkTracked`).
    pub chunks: &'a HashSet<(i32, i32)>,
}

/// Where tracker packets go: `(player, encoded packet with its id)`.
pub type Sink<'a> = dyn FnMut(ViewerKey, &[u8]) + 'a;

/// Entity types created with `dontTrackDeltas()` in vanilla's `EntityTypes`.
const NO_DELTA_TYPES: [&str; 11] = [
    "minecraft:bat",
    "minecraft:cushion",
    "minecraft:end_crystal",
    "minecraft:evoker_fangs",
    "minecraft:glow_item_frame",
    "minecraft:item_frame",
    "minecraft:leash_knot",
    "minecraft:llama_spit",
    "minecraft:painting",
    "minecraft:wither",
    "minecraft:player",
];

/// Tracking parameters of an entity type.
#[derive(Debug, Clone, Copy)]
pub struct TrackingInfo {
    /// Range in blocks (`clientTrackingRange * 16`); zero means untracked.
    pub range: i32,
    /// Update interval in ticks; `None` never updates on a schedule.
    pub interval: Option<u32>,
    pub track_deltas: bool,
    pub is_item: bool,
}

impl TrackingInfo {
    pub fn of(kind: u16) -> Self {
        if usize::from(kind) >= ferrite_data::entity_props::ENTITY_TYPE_COUNT {
            return Self {
                range: 0,
                interval: None,
                track_deltas: false,
                is_item: false,
            };
        }
        let props = ferrite_data::entity_props::get(kind);
        Self {
            range: props.client_tracking_range * 16,
            interval: u32::try_from(props.update_interval)
                .ok()
                .filter(|i| *i > 0 && props.update_interval != i32::MAX),
            track_deltas: !NO_DELTA_TYPES.contains(&props.key),
            is_item: props.key == "minecraft:item",
        }
    }
}

#[derive(Debug, Clone)]
struct Pairing {
    viewer: ViewerKey,
    absent: u8,
}

/// Per-entity tracker state (vanilla `ServerEntity` plus the `seenBy` set).
#[derive(Debug, Clone, Default)]
pub struct TrackerState {
    seen_by: Vec<Pairing>,
    started: bool,
    base: Vec3,
    last_yaw: u8,
    last_pitch: u8,
    last_head_yaw: u8,
    last_motion: Vec3,
    tick_count: u32,
    teleport_delay: u32,
    was_on_ground: bool,
}

impl TrackerState {
    /// Players currently tracking the entity.
    pub fn viewers(&self) -> impl Iterator<Item = ViewerKey> + '_ {
        self.seen_by.iter().map(|p| p.viewer)
    }

    pub fn is_seen_by(&self, viewer: ViewerKey) -> bool {
        self.seen_by.iter().any(|p| p.viewer == viewer)
    }
}

/// Vanilla `Mth.packDegrees`.
pub fn pack_degrees(angle: f32) -> u8 {
    (angle * 256.0 / 360.0).floor() as i32 as u8
}

/// Vanilla `VecDeltaCodec.encode`: Java `Math.round(v * 4096)`.
fn quantize(v: f64) -> i64 {
    (v * 4096.0 + 0.5).floor() as i64
}

fn precision_loss(v: f64) -> f64 {
    quantize(v) as f64 / 4096.0 - v
}

fn encode<P: Packet>(packet: &P) -> Vec<u8> {
    let mut out = Vec::new();
    encode_packet(packet, &mut out);
    out
}

/// Below this many viewers a plain scan is cheaper than the grid.
const GRID_MIN_VIEWERS: usize = 16;
const GRID_CELL: f64 = 64.0;

/// Viewers bucketed by position, in `GRID_CELL`-block cells.
struct ViewerGrid {
    all: usize,
    cells: std::collections::HashMap<(i32, i32), Vec<usize>>,
    index_of: std::collections::HashMap<ViewerKey, usize>,
}

impl ViewerGrid {
    fn cell(x: f64) -> i32 {
        (x / GRID_CELL).floor() as i32
    }

    fn new(viewers: &[Viewer<'_>]) -> Self {
        let mut grid = Self {
            all: viewers.len(),
            cells: Default::default(),
            index_of: Default::default(),
        };
        if viewers.len() >= GRID_MIN_VIEWERS {
            for (i, viewer) in viewers.iter().enumerate() {
                grid.cells
                    .entry((Self::cell(viewer.pos.x), Self::cell(viewer.pos.z)))
                    .or_default()
                    .push(i);
                grid.index_of.insert(viewer.key, i);
            }
        }
        grid
    }

    /// Indexes (ascending, so packets keep the viewer order) of the viewers within `range` of
    /// `pos` or already paired with the entity.
    fn candidates(&self, pos: Vec3, range: i32, seen_by: &[Pairing]) -> Vec<usize> {
        if self.cells.is_empty() {
            return (0..self.all).collect();
        }
        let range = f64::from(range);
        let mut found = Vec::new();
        for x in Self::cell(pos.x - range)..=Self::cell(pos.x + range) {
            for z in Self::cell(pos.z - range)..=Self::cell(pos.z + range) {
                if let Some(cell) = self.cells.get(&(x, z)) {
                    found.extend_from_slice(cell);
                }
            }
        }
        found.extend(
            seen_by
                .iter()
                .filter_map(|pairing| self.index_of.get(&pairing.viewer).copied()),
        );
        found.sort_unstable();
        found.dedup();
        found
    }
}

/// Runs one tracker tick over every entity in `store` for the players of its region.
pub fn track(store: &mut EntityStore, viewers: &[Viewer<'_>], sink: &mut Sink<'_>) {
    // Looked up once per pairing per entity; a linear scan made a tick quadratic in the players.
    let present: HashSet<ViewerKey> = viewers.iter().map(|v| v.key).collect();
    // With many viewers an entity only needs the ones near it (plus those it already shows
    // itself to, so they can be told to stop); a grid of viewers finds them without a full scan.
    let grid = ViewerGrid::new(viewers);
    let keys = store.keys().to_vec();
    for key in keys {
        let Some(i) = store.dense(key) else { continue };
        let pos = store.positions()[i];
        let vel = store.velocities()[i];
        let mut flags = store.all_flags()[i];
        let data = store.data_mut(key).unwrap();
        let info = TrackingInfo::of(data.kind);
        if info.range == 0 {
            continue;
        }
        let state = &mut data.tracker;
        if !state.started {
            state.started = true;
            state.base = pos;
            state.last_motion = vel;
            state.last_yaw = pack_degrees(data.yaw);
            state.last_pitch = pack_degrees(data.pitch);
            state.last_head_yaw = pack_degrees(data.head_yaw);
            state.was_on_ground = flags.has(Flags::ON_GROUND);
        }
        if flags.has(Flags::REMOVED) {
            let packet = encode(&RemoveEntities {
                ids: vec![VarInt(data.id)],
            });
            for pairing in data.tracker.seen_by.drain(..) {
                sink(pairing.viewer, &packet);
            }
            continue;
        }
        // Pairing (vanilla `TrackedEntity.updatePlayer`).
        let chunk = chunk_of(pos);
        let seen_now: HashSet<ViewerKey> = data.tracker.seen_by.iter().map(|p| p.viewer).collect();
        let mut stopped: HashSet<ViewerKey> = HashSet::new();
        let mut kept: HashSet<ViewerKey> = HashSet::new();
        let mut started = Vec::new();
        for vi in grid.candidates(pos, info.range, &data.tracker.seen_by) {
            let viewer = &viewers[vi];
            let dx = viewer.pos.x - pos.x;
            let dz = viewer.pos.z - pos.z;
            let range = f64::from(info.range.min(viewer.view_distance * 16));
            let visible = dx * dx + dz * dz <= range * range && viewer.chunks.contains(&chunk);
            match (seen_now.contains(&viewer.key), visible) {
                (false, true) => {
                    sink(
                        viewer.key,
                        &encode(&AddEntity {
                            id: VarInt(data.id),
                            uuid: data.uuid,
                            entity_type: VarInt(i32::from(data.kind)),
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                            velocity: LpVec3([
                                data.tracker.last_motion.x,
                                data.tracker.last_motion.y,
                                data.tracker.last_motion.z,
                            ]),
                            pitch: pack_degrees(data.pitch),
                            yaw: pack_degrees(data.yaw),
                            head_yaw: pack_degrees(data.head_yaw),
                            data: VarInt(data.spawn_data),
                        }),
                    );
                    let values = data.metadata.non_default();
                    if !values.is_empty() {
                        sink(
                            viewer.key,
                            &encode(&SetEntityData {
                                id: VarInt(data.id),
                                values,
                            }),
                        );
                    }
                    // Vanilla `ServerEntity.sendPairingData` sends a living entity's syncable
                    // attributes before its equipment.
                    let attributes = default_attributes(data);
                    if !attributes.is_empty() {
                        sink(
                            viewer.key,
                            &encode(&UpdateAttributes {
                                entity_id: VarInt(data.id),
                                attributes,
                            }),
                        );
                    }
                    // A viewer that starts tracking later still needs the entity's active
                    // effects; the trader's night invisibility is one of them.
                    for effect in data.effects.iter() {
                        sink(
                            viewer.key,
                            &encode(&UpdateMobEffect {
                                entity_id: VarInt(data.id),
                                effect: VarInt(effect.effect),
                                amplifier: VarInt(effect.amplifier),
                                duration: VarInt(effect.duration),
                                flags: effect.flags(),
                            }),
                        );
                    }
                    if let Some(state) = data.mob {
                        // What a horse wears and the lead it hangs on, for a viewer who just
                        // started tracking it.
                        if state.saddled || state.armor != 0 {
                            let saddle = if state.saddled {
                                ferrite_item::ItemStack::from_name("minecraft:saddle")
                                    .unwrap_or_default()
                            } else {
                                ferrite_item::ItemStack::empty()
                            };
                            let armor = if state.armor != 0 {
                                ferrite_item::ItemStack::with_count(state.armor, 1)
                            } else {
                                ferrite_item::ItemStack::empty()
                            };
                            sink(
                                viewer.key,
                                &encode(&SetEquipment {
                                    entity_id: VarInt(data.id),
                                    slots: vec![
                                        (crate::horse::SADDLE_SLOT, saddle),
                                        (crate::horse::BODY_SLOT, armor),
                                    ],
                                }),
                            );
                        }
                        if let Some((_, holder)) = state.leash {
                            sink(
                                viewer.key,
                                &encode(&SetEntityLink {
                                    attached: data.id,
                                    holder,
                                }),
                            );
                        }
                    }
                    started.push(viewer.key);
                }
                (true, false) => {
                    stopped.insert(viewer.key);
                    sink(
                        viewer.key,
                        &encode(&RemoveEntities {
                            ids: vec![VarInt(data.id)],
                        }),
                    );
                }
                (true, true) => {
                    kept.insert(viewer.key);
                }
                (false, false) => {}
            }
        }
        if !stopped.is_empty() {
            data.tracker
                .seen_by
                .retain(|p| !stopped.contains(&p.viewer));
        }
        for pairing in &mut data.tracker.seen_by {
            if kept.contains(&pairing.viewer) {
                pairing.absent = 0;
            }
        }
        data.tracker.seen_by.extend(
            started
                .into_iter()
                .map(|viewer| Pairing { viewer, absent: 0 }),
        );
        // Players missing from the region: keep them through a hand-off, then let go.
        let remove = encode(&RemoveEntities {
            ids: vec![VarInt(data.id)],
        });
        data.tracker.seen_by.retain_mut(|pairing| {
            if present.contains(&pairing.viewer) {
                return true;
            }
            pairing.absent += 1;
            if pairing.absent > ABSENT_GRACE_TICKS {
                sink(pairing.viewer, &remove);
                return false;
            }
            true
        });
        send_changes(data, pos, vel, &mut flags, info, &present, sink);
        *store.flags_mut(key).unwrap() = flags;
    }
}

/// The syncable default attributes of a living entity. An equine's rolled movement speed and
/// jump strength replace the type's defaults, as `AbstractHorse.randomizeAttributes` does.
fn default_attributes(data: &crate::store::EntityData) -> Vec<AttributeSnapshot> {
    let Some(state) = data.mob else {
        return Vec::new();
    };
    ferrite_data::entity_props::get(data.kind)
        .attributes
        .iter()
        .filter_map(|(name, base)| {
            let attribute = ferrite_data::registries::id("minecraft:attribute", name)?;
            let short = name.strip_prefix("minecraft:").unwrap_or(name);
            let value = match short {
                "movement_speed" if state.speed > 0.0 => f64::from(state.speed),
                "jump_strength" if state.jump > 0.0 => f64::from(state.jump),
                _ => *base,
            };
            Some(AttributeSnapshot {
                attribute: VarInt(attribute),
                base: value,
                modifiers: Vec::new(),
            })
        })
        .collect()
}

/// Vanilla `ServerEntity.sendChanges`.
fn send_changes(
    data: &mut crate::store::EntityData,
    pos: Vec3,
    vel: Vec3,
    flags: &mut Flags,
    info: TrackingInfo,
    viewers: &HashSet<ViewerKey>,
    sink: &mut Sink<'_>,
) {
    let id = VarInt(data.id);
    let on_ground = flags.has(Flags::ON_GROUND);
    let present: Vec<ViewerKey> = data
        .tracker
        .seen_by
        .iter()
        .filter(|p| p.absent == 0 && viewers.contains(&p.viewer))
        .map(|p| p.viewer)
        .collect();
    let mut broadcast = |packet: Vec<u8>| {
        for viewer in &present {
            sink(*viewer, &packet);
        }
    };
    let state = &mut data.tracker;
    if flags.has(Flags::TELEPORTED) {
        flags.set(Flags::TELEPORTED, false);
        broadcast(encode(&TeleportEntity {
            id,
            x: pos.x,
            y: pos.y,
            z: pos.z,
            velocity_x: vel.x,
            velocity_y: vel.y,
            velocity_z: vel.z,
            yaw: data.yaw,
            pitch: data.pitch,
            relatives: 0,
            on_ground,
        }));
        state.base = pos;
        state.last_yaw = pack_degrees(data.yaw);
        state.last_pitch = pack_degrees(data.pitch);
        state.was_on_ground = on_ground;
        state.teleport_delay = 0;
    }
    let scheduled = info
        .interval
        .is_some_and(|i| state.tick_count.is_multiple_of(i));
    if flags.has(Flags::NEEDS_SYNC) || scheduled || data.metadata.is_dirty() {
        let yaw = pack_degrees(data.yaw);
        let pitch = pack_degrees(data.pitch);
        let send_rotation = yaw != state.last_yaw || pitch != state.last_pitch;
        state.teleport_delay += 1;
        let changed = (pos - state.base).length_sqr() >= POSITION_TOLERANCE;
        let send_position = changed || state.tick_count.is_multiple_of(FORCED_POS_UPDATE_PERIOD);
        let delta = [
            quantize(pos.x) - quantize(state.base.x),
            quantize(pos.y) - quantize(state.base.y),
            quantize(pos.z) - quantize(state.base.z),
        ];
        let fits = delta.iter().all(|d| i16::try_from(*d).is_ok());
        let precise = info.is_item && {
            let (has_x, has_z) = (delta[0] != 0, delta[2] != 0);
            (flags.has(Flags::VERTICAL_COLLISION)
                && (has_x && precision_loss(pos.x) != 0.0 || has_z && precision_loss(pos.z) != 0.0))
                || (flags.has(Flags::HORIZONTAL_COLLISION)
                    && (has_x && precision_loss(pos.z) != 0.0
                        || has_z && precision_loss(pos.x) != 0.0))
        };
        // (packet, has position, has rotation)
        let movement = if state.teleport_delay > FORCED_TELEPORT_PERIOD
            || state.was_on_ground != on_ground
            || (send_position && (!fits || precise))
        {
            state.was_on_ground = on_ground;
            state.teleport_delay = 0;
            Some((
                encode(&EntityTeleport {
                    id,
                    path_type: VarInt(0),
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    yaw: data.yaw,
                    pitch: data.pitch,
                    on_ground,
                }),
                true,
                true,
            ))
        } else if send_position {
            let properties = VarInt(i32::from(on_ground));
            let (dx, dy, dz) = (delta[0] as i16, delta[1] as i16, delta[2] as i16);
            Some(if send_rotation {
                (
                    encode(&EntityMoveRotation {
                        id,
                        properties,
                        dx,
                        dy,
                        dz,
                        yaw,
                        pitch,
                    }),
                    true,
                    true,
                )
            } else {
                (
                    encode(&EntityMove {
                        id,
                        properties,
                        dx,
                        dy,
                        dz,
                    }),
                    true,
                    false,
                )
            })
        } else if send_rotation {
            Some((
                encode(&EntityRotation {
                    id,
                    on_ground,
                    yaw,
                    pitch,
                }),
                false,
                true,
            ))
        } else {
            None
        };
        if flags.has(Flags::NEEDS_SYNC) || info.track_deltas {
            let diff = (vel - state.last_motion).length_sqr();
            if diff > 1.0E-7 || diff > 0.0 && vel.length_sqr() == 0.0 {
                state.last_motion = vel;
                broadcast(encode(&SetEntityMotion {
                    id,
                    velocity: LpVec3([vel.x, vel.y, vel.z]),
                }));
            }
        }
        if let Some((packet, has_position, has_rotation)) = movement {
            broadcast(packet);
            if has_position {
                state.base = pos;
            }
            if has_rotation {
                state.last_yaw = yaw;
                state.last_pitch = pitch;
            }
        }
        if let Some(values) = data.metadata.pack_dirty() {
            broadcast(encode(&SetEntityData { id, values }));
        }
        let head = pack_degrees(data.head_yaw);
        if head != state.last_head_yaw {
            broadcast(encode(&EntityHeadRotation { id, yaw: head }));
            state.last_head_yaw = head;
        }
        flags.set(Flags::NEEDS_SYNC, false);
    }
    state.tick_count += 1;
}

/// Moves an entity with an explicit teleport (`Entity.teleportTo`): the next tracker tick
/// sends `teleport_entity` instead of a movement delta.
pub fn teleport(store: &mut EntityStore, key: crate::EntityKey, pos: Vec3, vel: Vec3) {
    if store.data(key).is_none() {
        return;
    }
    store.set_pos(key, pos);
    store.set_vel(key, vel);
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::TELEPORTED, true);
        flags.set(Flags::NEEDS_SYNC, true);
    }
}

/// Flags an entity for removal, so its trackers receive `remove_entities` on the next tick.
pub fn remove(store: &mut EntityStore, key: crate::EntityKey) {
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::REMOVED, true);
    }
}

/// Sends a packet to every player tracking an entity (vanilla `sendToTrackingPlayers`).
pub fn broadcast<P: Packet>(
    store: &EntityStore,
    key: crate::EntityKey,
    packet: &P,
    sink: &mut Sink<'_>,
) {
    if let Some(data) = store.data(key) {
        let bytes = encode(packet);
        for viewer in data.tracker.viewers() {
            sink(viewer, &bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{Effect, MobEffectInstance};
    use crate::id::{allocate_id, random_uuid};
    use crate::metadata::item::ITEM;
    use crate::store::{EntityData, EntityRecord};
    use ferrite_protocol::{
        Decode,
        packets::play::{ItemStack, MetaValue, UpdateMobEffect},
    };

    fn item_kind() -> u16 {
        ferrite_data::entity_props::by_name("item").unwrap().id
    }

    fn spawn(store: &mut EntityStore, pos: Vec3) -> crate::EntityKey {
        let mut data = EntityData::new(allocate_id(), random_uuid(), item_kind(), 0.25, 0.25);
        data.metadata
            .define(ITEM, MetaValue::ItemStack(ItemStack::default()));
        data.metadata.set(
            ITEM,
            MetaValue::ItemStack(ItemStack {
                count: 1,
                item_id: 1,
            }),
        );
        store.insert(EntityRecord {
            pos,
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data,
        })
    }

    type Sent = Vec<(ViewerKey, i32, Vec<u8>)>;

    /// Runs one tracker tick and returns what each viewer received.
    fn tick(store: &mut EntityStore, viewers: &[Viewer]) -> Sent {
        let mut out = Vec::new();
        track(store, viewers, &mut |viewer, packet: &[u8]| {
            let mut body = packet;
            let id = VarInt::decode(&mut body).unwrap().0;
            out.push((viewer, id, body.to_vec()));
        });
        out
    }

    fn ids(packets: &Sent) -> Vec<i32> {
        packets.iter().map(|p| p.1).collect()
    }

    fn viewer(
        key: ViewerKey,
        x: f64,
        z: f64,
        view: i32,
        chunks: &HashSet<(i32, i32)>,
    ) -> Viewer<'_> {
        Viewer {
            key,
            pos: Vec3::new(x, 64.0, z),
            view_distance: view,
            chunks,
        }
    }

    #[test]
    fn pairs_by_range_and_loaded_chunk() {
        let mut store = EntityStore::default();
        spawn(&mut store, Vec3::new(8.0, 64.0, 8.0));
        let chunks = HashSet::from([(0, 0)]);
        let none = HashSet::new();
        let near = (Uuid::from_u128(1), 0);
        let viewers = [
            viewer(near, 10.0, 10.0, 10, &chunks),
            // The entity's chunk was never sent to this player.
            viewer((Uuid::from_u128(2), 0), 10.0, 10.0, 10, &none),
            // Item range is 6 chunks = 96 blocks.
            viewer((Uuid::from_u128(3), 0), 8.0, 105.0, 10, &chunks),
        ];
        let packets = tick(&mut store, &viewers);
        // Pairing first; then, like vanilla, the tick-0 forced position and the data that is
        // still dirty from spawning.
        assert_eq!(
            ids(&packets),
            [
                AddEntity::ID,
                SetEntityData::ID,
                EntityMove::ID,
                SetEntityData::ID
            ]
        );
        assert!(packets.iter().all(|p| p.0 == near));
        let add = AddEntity::decode(&mut packets[0].2.as_slice()).unwrap();
        assert_eq!(add.entity_type.0, i32::from(item_kind()));
        let data = SetEntityData::decode(&mut packets[1].2.as_slice()).unwrap();
        assert_eq!(data.values.len(), 1);
        assert_eq!(data.values[0].index, ITEM);
        // A short view distance caps the range; walking out of it removes the entity.
        let viewers = [viewer(near, 8.0, 48.0, 2, &chunks)];
        assert_eq!(ids(&tick(&mut store, &viewers)), [RemoveEntities::ID]);
    }

    #[test]
    fn a_living_entity_sends_its_default_attributes_when_a_viewer_starts_tracking() {
        let mut store = EntityStore::default();
        let zombie = crate::mob::new_mob(
            crate::mob::MobKind::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(1),
        );
        let entity_id = zombie.data.id;
        store.insert(zombie);
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let update = packets
            .iter()
            .find(|p| p.1 == UpdateAttributes::ID)
            .expect("a living entity sends its attributes");
        let update = UpdateAttributes::decode(&mut update.2.as_slice()).unwrap();
        assert_eq!(update.entity_id.0, entity_id);
        let health = update
            .attributes
            .iter()
            .find(|attribute| {
                Some(attribute.attribute.0)
                    == ferrite_data::registries::id("minecraft:attribute", "minecraft:max_health")
            })
            .expect("max health");
        assert!(
            (health.base - 20.0).abs() < 1e-9,
            "zombie health: {}",
            health.base
        );
        assert!(
            update
                .attributes
                .iter()
                .any(|attribute| Some(attribute.attribute.0)
                    == ferrite_data::registries::id(
                        "minecraft:attribute",
                        "minecraft:movement_speed"
                    )),
            "movement speed is synched"
        );
        // Items have no attributes, so they send no attribute packet.
        let mut store = EntityStore::default();
        spawn(&mut store, Vec3::new(8.0, 64.0, 8.0));
        assert!(
            tick(&mut store, &viewers)
                .iter()
                .all(|p| p.1 != UpdateAttributes::ID)
        );
    }

    #[test]
    fn a_bobber_spawn_packet_carries_its_owners_entity_id() {
        let mut store = EntityStore::default();
        let record = crate::bobber::cast(
            Uuid::from_u128(9),
            41,
            Vec3::new(8.0, 64.0, 8.0),
            0.0,
            0.0,
            0,
            &mut crate::Rng::new(1),
        );
        let kind = record.data.kind;
        store.insert(record);
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let add = packets
            .iter()
            .find(|p| p.1 == AddEntity::ID)
            .expect("the bobber is spawned for a nearby viewer");
        let add = AddEntity::decode(&mut add.2.as_slice()).unwrap();
        assert_eq!(add.entity_type.0, i32::from(kind));
        assert_eq!(add.data.0, 41);
    }

    /// Past `GRID_MIN_VIEWERS` the grid picks the candidates; it must pair the near viewers, skip
    /// the far ones (a cell away included) and still tell a viewer that walked off to stop.
    #[test]
    fn the_viewer_grid_pairs_the_same_viewers_as_a_full_scan() {
        let mut store = EntityStore::default();
        spawn(&mut store, Vec3::new(8.0, 64.0, 8.0));
        let chunks = HashSet::from([(0, 0)]);
        let key = |n: u128| (Uuid::from_u128(n), 0);
        let mut viewers: Vec<Viewer> = (0..GRID_MIN_VIEWERS as u128)
            .map(|n| viewer(key(n), 1000.0 + 100.0 * n as f64, 1000.0, 10, &chunks))
            .collect();
        viewers.push(viewer(key(100), 10.0, 10.0, 10, &chunks));
        // Item range is 96 blocks: this one is in the next grid cell but inside the range.
        viewers.push(viewer(key(101), 8.0, 90.0, 10, &chunks));
        let packets = tick(&mut store, &viewers);
        let seen: HashSet<ViewerKey> = packets
            .iter()
            .filter(|p| p.1 == AddEntity::ID)
            .map(|p| p.0)
            .collect();
        assert_eq!(seen, HashSet::from([key(100), key(101)]));
        // The first viewer leaves the range; the entity still has to remove itself from them.
        viewers[17] = viewer(key(101), 8.0, 400.0, 10, &chunks);
        let packets = tick(&mut store, &viewers);
        let removed: Vec<ViewerKey> = packets
            .iter()
            .filter(|p| p.1 == RemoveEntities::ID)
            .map(|p| p.0)
            .collect();
        assert_eq!(removed, [key(101)]);
    }

    #[test]
    fn movement_uses_deltas_then_full_sync() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(8.0, 64.0, 8.0));
        let chunks = HashSet::from([(0, 0), (1, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        tick(&mut store, &viewers);
        // Off-interval ticks send nothing even though the item moved.
        store.set_pos(key, Vec3::new(8.5, 64.0, 8.0));
        assert!(tick(&mut store, &viewers).is_empty());
        // A forced sync sends the delta (0.5 block = 2048 units) right away.
        store.flags_mut(key).unwrap().set(Flags::NEEDS_SYNC, true);
        let packets = tick(&mut store, &viewers);
        assert_eq!(ids(&packets), [EntityMove::ID]);
        let moved = EntityMove::decode(&mut packets[0].2.as_slice()).unwrap();
        assert_eq!((moved.dx, moved.dy, moved.dz), (2048, 0, 0));
        // A move longer than 8 blocks cannot be a delta.
        store.set_pos(key, Vec3::new(20.0, 64.0, 8.0));
        store.flags_mut(key).unwrap().set(Flags::NEEDS_SYNC, true);
        assert_eq!(ids(&tick(&mut store, &viewers)), [EntityTeleport::ID]);
        // Landing changes on-ground, which forces a full sync as well.
        let flags = store.flags_mut(key).unwrap();
        flags.set(Flags::ON_GROUND, true);
        flags.set(Flags::NEEDS_SYNC, true);
        assert_eq!(ids(&tick(&mut store, &viewers)), [EntityTeleport::ID]);
        // Items track velocity, and only changed data values are sent.
        store.set_vel(key, Vec3::new(0.0, 0.2, 0.0));
        store.data_mut(key).unwrap().metadata.set(
            ITEM,
            MetaValue::ItemStack(ItemStack {
                count: 2,
                item_id: 1,
            }),
        );
        let packets = tick(&mut store, &viewers);
        assert_eq!(ids(&packets), [SetEntityMotion::ID, SetEntityData::ID]);
        let data = SetEntityData::decode(&mut packets[1].2.as_slice()).unwrap();
        assert_eq!(data.values.len(), 1);
        // Explicit teleports use teleport_entity.
        store.set_pos(key, Vec3::new(9.0, 70.0, 9.0));
        store.flags_mut(key).unwrap().set(Flags::TELEPORTED, true);
        assert_eq!(ids(&tick(&mut store, &viewers)), [TeleportEntity::ID]);
    }

    #[test]
    fn active_effects_reach_a_viewer_that_starts_tracking() {
        use crate::mob::{MobKind, new_mob};
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(1),
        ));
        store
            .data_mut(key)
            .unwrap()
            .effects
            .add(MobEffectInstance::new(Effect::Invisibility.id(), 3600, 0));
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let effect = packets
            .iter()
            .find(|packet| packet.1 == UpdateMobEffect::ID)
            .expect("new viewer receives the active effect");
        let effect = UpdateMobEffect::decode(&mut effect.2.as_slice()).unwrap();
        assert_eq!(effect.entity_id.0, store.data(key).unwrap().id);
        assert_eq!(effect.effect.0, Effect::Invisibility.id());
        assert_eq!(effect.duration.0, 3600);
    }

    #[test]
    fn mobs_are_tracked_moved_teleported_and_removed() {
        use crate::mob::{MobKind, new_mob};
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(1),
        ));
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        assert_eq!(packets[0].1, AddEntity::ID);
        let add = AddEntity::decode(&mut packets[0].2.as_slice()).unwrap();
        assert_eq!(add.entity_type.0, i32::from(MobKind::Cow.entity_type()));
        // A cow updates every 3 ticks; a forced sync sends the delta right away.
        store.set_pos(key, Vec3::new(9.0, 64.0, 8.0));
        store.flags_mut(key).unwrap().set(Flags::NEEDS_SYNC, true);
        assert!(
            tick(&mut store, &viewers)
                .iter()
                .any(|packet| matches!(packet.1, EntityMove::ID | EntityMoveRotation::ID))
        );
        // An explicit teleport is not a movement delta.
        teleport(&mut store, key, Vec3::new(12.0, 66.0, 8.0), Vec3::ZERO);
        assert!(
            tick(&mut store, &viewers)
                .iter()
                .any(|packet| packet.1 == TeleportEntity::ID)
        );
        // Removal reaches the viewer.
        remove(&mut store, key);
        assert!(
            tick(&mut store, &viewers)
                .iter()
                .any(|packet| packet.1 == RemoveEntities::ID)
        );
    }

    #[test]
    fn absent_viewers_get_a_grace_period_and_removal_reaches_all() {
        let mut store = EntityStore::default();
        let key = spawn(&mut store, Vec3::new(8.0, 64.0, 8.0));
        let chunks = HashSet::from([(0, 0)]);
        let a = (Uuid::from_u128(1), 0);
        let b = (Uuid::from_u128(2), 0);
        let both = [
            viewer(a, 8.0, 8.0, 10, &chunks),
            viewer(b, 8.0, 8.0, 10, &chunks),
        ];
        tick(&mut store, &both);
        // `b` is in transit between regions: kept for the grace period, then removed.
        for _ in 0..ABSENT_GRACE_TICKS {
            assert!(tick(&mut store, &both[..1]).iter().all(|p| p.0 == a));
        }
        let packets = tick(&mut store, &both[..1]);
        assert_eq!(
            packets.iter().map(|p| (p.0, p.1)).collect::<Vec<_>>(),
            [(b, RemoveEntities::ID)]
        );
        // A reconnect is a new viewer and gets a fresh spawn.
        let again = [viewer((a.0, 1), 8.0, 8.0, 10, &chunks)];
        assert!(
            tick(&mut store, &again)
                .iter()
                .any(|p| p.1 == AddEntity::ID)
        );
        // Removal reaches everyone still tracking the entity.
        store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
        let packets = tick(&mut store, &both[..1]);
        assert_eq!(
            packets.iter().filter(|p| p.1 == RemoveEntities::ID).count(),
            2
        );
    }
}
