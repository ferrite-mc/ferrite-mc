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
        SetEntityMotion, SetEquipment, SetPassengers, TeleportEntity, UpdateAttributes,
        UpdateMobEffect,
    },
    types::LpVec3,
};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h = (h.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        self.0 = h;
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.0 = (self.0.rotate_left(5) ^ (i as u64)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    #[inline]
    fn write_u128(&mut self, i: u128) {
        let mut h = (self.0.rotate_left(5) ^ (i as u64)).wrapping_mul(0x517c_c1b7_2722_0a95);
        h = (h.rotate_left(5) ^ ((i >> 64) as u64)).wrapping_mul(0x517c_c1b7_2722_0a95);
        self.0 = h;
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// A tracking player: UUID plus a session number, so a reconnect is a new viewer.
pub type ViewerKey = (Uuid, u64);

/// Ticks a pairing survives while its player is missing from the region.
pub const ABSENT_GRACE_TICKS: u8 = 20;
/// Vanilla `ServerEntity.TOLERANCE_LEVEL_POSITION` (a float constant widened to double).
const POSITION_TOLERANCE: f64 = 7.629_394_5E-6_f32 as f64;
const FORCED_POS_UPDATE_PERIOD: u32 = 60;
const FORCED_TELEPORT_PERIOD: u32 = 400;

/// A player as the tracker sees it.
pub struct Viewer<'a, S = std::collections::hash_map::RandomState> {
    pub key: ViewerKey,
    pub pos: Vec3,
    /// View distance in chunks.
    pub view_distance: i32,
    /// Chunks already sent to the player (vanilla `isChunkTracked`).
    pub chunks: &'a HashSet<(i32, i32), S>,
}

/// Where tracker packets go: `(player, encoded packet with its id)`.
pub type Sink<'a> = dyn FnMut(ViewerKey, &[u8]) + 'a;

/// Zero-copy shared-buffer sink: `(player, Arc<[u8]> of encoded packet with its id)`.
pub type ArcSink<'a> = dyn FnMut(ViewerKey, &Arc<[u8]>) + 'a;

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
    /// Passenger network ids published by the owning region for `sendPairingData`. The ride
    /// table is keyed by UUID; the region resolves these each tick.
    pub passengers: Vec<i32>,
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

#[inline]
fn encode_arc<P: Packet>(packet: &P, buf: &mut Vec<u8>) -> Arc<[u8]> {
    buf.clear();
    encode_packet(packet, buf);
    Arc::from(buf.as_slice())
}

/// Below this many viewers a plain scan is cheaper than the grid.
const GRID_MIN_VIEWERS: usize = 16;
const GRID_CELL: f64 = 64.0;

/// Viewers bucketed by position, in `GRID_CELL`-block cells.
struct ViewerGrid {
    all: usize,
    cells: FastMap<(i32, i32), Vec<usize>>,
    index_of: FastMap<ViewerKey, usize>,
}

impl ViewerGrid {
    fn cell(x: f64) -> i32 {
        (x / GRID_CELL).floor() as i32
    }

    fn new<S>(viewers: &[Viewer<'_, S>]) -> Self {
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

    #[inline]
    fn index_of_viewer<S>(&self, viewers: &[Viewer<'_, S>], key: ViewerKey) -> Option<usize> {
        if self.cells.is_empty() {
            viewers.iter().position(|v| v.key == key)
        } else {
            self.index_of.get(&key).copied()
        }
    }

    /// Viewers within `range` of `pos`, plus previously paired viewers that must receive a
    /// removal. Each viewer is marked with `stamp` so overlapping grid cells need no sort or
    /// dedup; a dense region otherwise sorted the same viewers once per entity per tick.
    fn candidates_into(
        &self,
        pos: Vec3,
        range: i32,
        seen_by: &[Pairing],
        found: &mut Vec<usize>,
        stamps: &mut [u32],
        stamp: u32,
    ) {
        found.clear();
        if self.cells.is_empty() {
            for (vi, slot) in stamps.iter_mut().enumerate().take(self.all) {
                *slot = stamp;
                found.push(vi);
            }
        } else {
            let range = f64::from(range);
            for x in Self::cell(pos.x - range)..=Self::cell(pos.x + range) {
                for z in Self::cell(pos.z - range)..=Self::cell(pos.z + range) {
                    if let Some(cell) = self.cells.get(&(x, z)) {
                        for &vi in cell {
                            if stamps[vi] != stamp {
                                stamps[vi] = stamp;
                                found.push(vi);
                            }
                        }
                    }
                }
            }
        }
        for pairing in seen_by {
            if let Some(vi) = self.index_of.get(&pairing.viewer).copied()
                && stamps[vi] != stamp
            {
                stamps[vi] = stamp;
                found.push(vi);
            }
        }
    }
}

/// Runs one tracker tick over every entity in `store` for the players of its region.
pub fn track<S: std::hash::BuildHasher + Sync>(
    store: &mut EntityStore,
    viewers: &[Viewer<'_, S>],
    sink: &mut Sink<'_>,
) {
    track_arc(store, viewers, &mut |viewer, bytes| sink(viewer, bytes));
}

/// Zero-copy [`track`] sending shared `Arc<[u8]>` buffers.
pub fn track_arc<S: std::hash::BuildHasher + Sync>(
    store: &mut EntityStore,
    viewers: &[Viewer<'_, S>],
    sink: &mut ArcSink<'_>,
) {
    track_arc_batched(store, viewers, &mut |_vi, key, pkts| {
        for pkt in pkts {
            sink(key, pkt);
        }
    });
}

/// Parallelized, zero-copy [`track`] delivering per-viewer packet batches.
///
/// `on_batch(Some(vi), viewers[vi].key, pkts)` receives batches for `viewers[vi]`;
/// `on_batch(None, key, pkts)` receives remove packets for previously paired viewers
/// that are no longer in `viewers`.
pub fn track_arc_batched<S: std::hash::BuildHasher + Sync>(
    store: &mut EntityStore,
    viewers: &[Viewer<'_, S>],
    on_batch: &mut impl FnMut(Option<usize>, ViewerKey, &[Arc<[u8]>]),
) {
    if (viewers.is_empty() && store.is_empty()) || store.is_empty() {
        return;
    }
    let grid = ViewerGrid::new(viewers);
    let n_entities = store.len();
    let work = n_entities.saturating_mul(viewers.len());
    let (pos, vel, flags, data) = store.tracking_slices_mut();
    let threads = if work < 4096 || n_entities < 32 || viewers.len() < GRID_MIN_VIEWERS {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8)
            .min(n_entities / 16)
            .max(1)
    };
    if threads <= 1 {
        track_slice(
            pos,
            vel,
            flags,
            data,
            viewers,
            &grid,
            &mut |vi, key, pkt| on_batch(vi, key, std::slice::from_ref(pkt)),
        );
        return;
    }
    type TrackWorkerBatch = (Vec<Vec<Arc<[u8]>>>, Vec<(ViewerKey, Arc<[u8]>)>);
    let chunk_len = n_entities.div_ceil(threads);
    let results: Vec<TrackWorkerBatch> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for ((p_chunk, v_chunk), (f_chunk, d_chunk)) in pos
            .chunks(chunk_len)
            .zip(vel.chunks(chunk_len))
            .zip(flags.chunks_mut(chunk_len).zip(data.chunks_mut(chunk_len)))
        {
            let grid_ref = &grid;
            handles.push(scope.spawn(move || {
                let mut per_viewer: Vec<Vec<Arc<[u8]>>> = vec![Vec::new(); viewers.len()];
                let mut offline: Vec<(ViewerKey, Arc<[u8]>)> = Vec::new();
                track_slice(
                    p_chunk,
                    v_chunk,
                    f_chunk,
                    d_chunk,
                    viewers,
                    grid_ref,
                    &mut |vi, key, pkt| {
                        if let Some(vi) = vi {
                            per_viewer[vi].push(Arc::clone(pkt));
                        } else {
                            offline.push((key, Arc::clone(pkt)));
                        }
                    },
                );
                (per_viewer, offline)
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().expect("tracker worker panicked"))
            .collect()
    });
    for (per_viewer, offline) in results {
        for (vi, pkts) in per_viewer.iter().enumerate() {
            if !pkts.is_empty() {
                on_batch(Some(vi), viewers[vi].key, pkts);
            }
        }
        for (key, pkt) in &offline {
            on_batch(None, *key, std::slice::from_ref(pkt));
        }
    }
}

fn build_pairing_packets(
    data: &mut crate::store::EntityData,
    pos: Vec3,
    pkt_buf: &mut Vec<u8>,
    paired_equipment: &mut Vec<(u8, ferrite_item::ItemStack)>,
) -> Vec<Arc<[u8]>> {
    let mut packets = Vec::with_capacity(4);
    packets.push(encode_arc(
        &AddEntity {
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
        },
        pkt_buf,
    ));
    let values = data.metadata.non_default();
    if !values.is_empty() {
        packets.push(encode_arc(
            &SetEntityData {
                id: VarInt(data.id),
                values,
            },
            pkt_buf,
        ));
    }
    // Vanilla `ServerEntity.sendPairingData` sends a living entity's syncable
    // attributes before its equipment.
    let attributes = default_attributes(data);
    if !attributes.is_empty() {
        packets.push(encode_arc(
            &UpdateAttributes {
                entity_id: VarInt(data.id),
                attributes,
            },
            pkt_buf,
        ));
    }
    // A viewer that starts tracking later still needs the entity's active
    // effects; the trader's night invisibility is one of them.
    for effect in data.effects.iter() {
        packets.push(encode_arc(
            &UpdateMobEffect {
                entity_id: VarInt(data.id),
                effect: VarInt(effect.effect),
                amplifier: VarInt(effect.amplifier),
                duration: VarInt(effect.duration),
                flags: effect.flags(),
            },
            pkt_buf,
        ));
    }
    paired_equipment.clear();
    for slot in crate::EquipmentSlot::ALL {
        if let Some(stack) = data.equipment.get(slot) {
            paired_equipment.push((slot.as_wire_slot(), stack.clone()));
        } else if let Some(state) = data.mob {
            let idx = slot.as_index();
            if idx < crate::mob::EQUIPMENT_SLOTS {
                let item = state.equipment[idx];
                if item != 0 {
                    paired_equipment.push((
                        slot.as_wire_slot(),
                        ferrite_item::ItemStack::with_count(item, 1),
                    ));
                }
            } else if slot == crate::EquipmentSlot::Body
                && crate::mob::can_use_equipment_slot(state.kind, slot)
                && state.armor != 0
            {
                paired_equipment.push((
                    slot.as_wire_slot(),
                    ferrite_item::ItemStack::with_count(state.armor, 1),
                ));
            } else if slot == crate::EquipmentSlot::Saddle
                && crate::mob::can_use_equipment_slot(state.kind, slot)
                && state.saddled
                && let Some(saddle) = ferrite_item::ItemStack::from_name("minecraft:saddle")
            {
                paired_equipment.push((slot.as_wire_slot(), saddle));
            }
        }
    }
    if !paired_equipment.is_empty() {
        packets.push(encode_arc(
            &SetEquipment {
                entity_id: VarInt(data.id),
                slots: std::mem::take(paired_equipment),
            },
            pkt_buf,
        ));
    }
    if let Some(state) = data.mob
        && let Some((_, holder)) = state.leash
    {
        packets.push(encode_arc(
            &SetEntityLink {
                attached: data.id,
                holder,
            },
            pkt_buf,
        ));
    }
    if !data.tracker.passengers.is_empty() {
        packets.push(encode_arc(
            &SetPassengers {
                vehicle: VarInt(data.id),
                passengers: data
                    .tracker
                    .passengers
                    .iter()
                    .copied()
                    .map(VarInt)
                    .collect(),
            },
            pkt_buf,
        ));
    }
    packets
}

#[allow(clippy::too_many_arguments)]
fn track_slice<S: std::hash::BuildHasher>(
    positions: &[Vec3],
    velocities: &[Vec3],
    flags_slice: &mut [Flags],
    data_slice: &mut [crate::store::EntityData],
    viewers: &[Viewer<'_, S>],
    grid: &ViewerGrid,
    emit: &mut impl FnMut(Option<usize>, ViewerKey, &Arc<[u8]>),
) {
    let n_viewers = viewers.len();
    let mut seen_stamp: Vec<u32> = vec![0; n_viewers];
    let mut kept_stamp: Vec<u32> = vec![0; n_viewers];
    let mut stopped_stamp: Vec<u32> = vec![0; n_viewers];
    let mut candidate_stamp: Vec<u32> = vec![0; n_viewers];
    let mut stamp: u32 = 0;
    let mut started: Vec<ViewerKey> = Vec::new();
    let mut candidate_buf: Vec<usize> = Vec::new();
    let mut present_buf: Vec<usize> = Vec::new();
    let mut pkt_buf: Vec<u8> = Vec::with_capacity(256);
    let mut paired_equipment: Vec<(u8, ferrite_item::ItemStack)> = Vec::with_capacity(6);
    // Cache chunk -> viewer-loaded bitset per slice so entities sharing a chunk do not
    // re-hash `(cx, cz)` across hundreds of viewers on every entity.
    let vis_words = n_viewers.div_ceil(64);
    let mut chunk_vis: FastMap<(i32, i32), Vec<u64>> = FastMap::default();

    for i in 0..data_slice.len() {
        let pos = positions[i];
        let vel = velocities[i];
        let mut flags = flags_slice[i];
        let data = &mut data_slice[i];
        let info = TrackingInfo::of(data.kind);
        if info.range == 0 {
            continue;
        }
        if viewers.is_empty() && data.tracker.seen_by.is_empty() {
            continue;
        }
        crate::mob::sync_state_metadata(data);
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
            if !data.tracker.seen_by.is_empty() {
                let packet = encode_arc(
                    &RemoveEntities {
                        ids: vec![VarInt(data.id)],
                    },
                    &mut pkt_buf,
                );
                for pairing in data.tracker.seen_by.drain(..) {
                    let vi = grid.index_of_viewer(viewers, pairing.viewer);
                    emit(vi, pairing.viewer, &packet);
                }
            }
            continue;
        }
        // Pairing (vanilla `TrackedEntity.updatePlayer`).
        let chunk = chunk_of(pos);
        stamp = stamp.wrapping_add(1);
        if stamp == 0 {
            seen_stamp.fill(0);
            kept_stamp.fill(0);
            stopped_stamp.fill(0);
            candidate_stamp.fill(0);
            stamp = 1;
        }
        for pairing in &data.tracker.seen_by {
            if let Some(vi) = grid.index_of_viewer(viewers, pairing.viewer) {
                seen_stamp[vi] = stamp;
            }
        }
        started.clear();
        present_buf.clear();
        grid.candidates_into(
            pos,
            info.range,
            &data.tracker.seen_by,
            &mut candidate_buf,
            &mut candidate_stamp,
            stamp,
        );
        let mut pairing_packets: Option<Vec<Arc<[u8]>>> = None;
        let mut remove_packet: Option<Arc<[u8]>> = None;
        let vis_bits: Option<&[u64]> = if n_viewers >= GRID_MIN_VIEWERS {
            let bits = chunk_vis.entry(chunk).or_insert_with(|| {
                let mut words = vec![0u64; vis_words];
                for (vi, v) in viewers.iter().enumerate() {
                    if v.chunks.contains(&chunk) {
                        words[vi >> 6] |= 1u64 << (vi & 63);
                    }
                }
                words
            });
            Some(bits.as_slice())
        } else {
            None
        };
        for &vi in &candidate_buf {
            let viewer = &viewers[vi];
            let dx = viewer.pos.x - pos.x;
            let dz = viewer.pos.z - pos.z;
            let range = f64::from(info.range.min(viewer.view_distance * 16));
            let chunk_loaded = match vis_bits {
                Some(words) => (words[vi >> 6] >> (vi & 63)) & 1 != 0,
                None => viewer.chunks.contains(&chunk),
            };
            let visible = dx * dx + dz * dz <= range * range && chunk_loaded;
            match (seen_stamp[vi] == stamp, visible) {
                (false, true) => {
                    let pkts = pairing_packets.get_or_insert_with(|| {
                        build_pairing_packets(data, pos, &mut pkt_buf, &mut paired_equipment)
                    });
                    for pkt in pkts.iter() {
                        emit(Some(vi), viewer.key, pkt);
                    }
                    started.push(viewer.key);
                    present_buf.push(vi);
                }
                (true, false) => {
                    stopped_stamp[vi] = stamp;
                    let pkt = remove_packet.get_or_insert_with(|| {
                        encode_arc(
                            &RemoveEntities {
                                ids: vec![VarInt(data.id)],
                            },
                            &mut pkt_buf,
                        )
                    });
                    emit(Some(vi), viewer.key, pkt);
                }
                (true, true) => {
                    kept_stamp[vi] = stamp;
                    present_buf.push(vi);
                }
                (false, false) => {}
            }
        }
        // Players missing from the region: keep them through a hand-off, then let go.
        if !data.tracker.seen_by.is_empty() {
            data.tracker.seen_by.retain_mut(|pairing| {
                if let Some(vi) = grid.index_of_viewer(viewers, pairing.viewer) {
                    if stopped_stamp[vi] == stamp {
                        return false;
                    }
                    if kept_stamp[vi] == stamp {
                        pairing.absent = 0;
                    }
                    true
                } else {
                    pairing.absent += 1;
                    if pairing.absent > ABSENT_GRACE_TICKS {
                        let pkt = remove_packet.get_or_insert_with(|| {
                            encode_arc(
                                &RemoveEntities {
                                    ids: vec![VarInt(data.id)],
                                },
                                &mut pkt_buf,
                            )
                        });
                        emit(None, pairing.viewer, pkt);
                        return false;
                    }
                    true
                }
            });
        }
        data.tracker.seen_by.extend(
            started
                .drain(..)
                .map(|viewer| Pairing { viewer, absent: 0 }),
        );
        send_changes_arc(
            data,
            pos,
            vel,
            &mut flags,
            info,
            &present_buf,
            &mut pkt_buf,
            &mut |vi, pkt| emit(Some(vi), viewers[vi].key, pkt),
        );
        flags_slice[i] = flags;
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
#[allow(clippy::too_many_arguments)]
fn send_changes_arc(
    data: &mut crate::store::EntityData,
    pos: Vec3,
    vel: Vec3,
    flags: &mut Flags,
    info: TrackingInfo,
    present: &[usize],
    pkt_buf: &mut Vec<u8>,
    emit_viewer: &mut impl FnMut(usize, &Arc<[u8]>),
) {
    let id = VarInt(data.id);
    let on_ground = flags.has(Flags::ON_GROUND);
    let mut broadcast_pkt = |packet: &Arc<[u8]>| {
        for &vi in present {
            emit_viewer(vi, packet);
        }
    };
    crate::mob::sync_state_metadata(data);
    let state = &mut data.tracker;
    if flags.has(Flags::TELEPORTED) {
        flags.set(Flags::TELEPORTED, false);
        if !present.is_empty() {
            let pkt = encode_arc(
                &TeleportEntity {
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
                },
                pkt_buf,
            );
            broadcast_pkt(&pkt);
        }
        state.base = pos;
        state.last_yaw = pack_degrees(data.yaw);
        state.last_pitch = pack_degrees(data.pitch);
        state.was_on_ground = on_ground;
        state.teleport_delay = 0;
    }
    let scheduled = info
        .interval
        .is_some_and(|i| state.tick_count.is_multiple_of(i));
    if flags.has(Flags::NEEDS_SYNC)
        || scheduled
        || data.metadata.is_dirty()
        || data.equipment.is_dirty()
    {
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
                (!present.is_empty()).then(|| {
                    encode_arc(
                        &EntityTeleport {
                            id,
                            path_type: VarInt(0),
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                            yaw: data.yaw,
                            pitch: data.pitch,
                            on_ground,
                        },
                        pkt_buf,
                    )
                }),
                true,
                true,
            ))
        } else if send_position {
            let properties = VarInt(i32::from(on_ground));
            let (dx, dy, dz) = (delta[0] as i16, delta[1] as i16, delta[2] as i16);
            Some(if send_rotation {
                (
                    (!present.is_empty()).then(|| {
                        encode_arc(
                            &EntityMoveRotation {
                                id,
                                properties,
                                dx,
                                dy,
                                dz,
                                yaw,
                                pitch,
                            },
                            pkt_buf,
                        )
                    }),
                    true,
                    true,
                )
            } else {
                (
                    (!present.is_empty()).then(|| {
                        encode_arc(
                            &EntityMove {
                                id,
                                properties,
                                dx,
                                dy,
                                dz,
                            },
                            pkt_buf,
                        )
                    }),
                    true,
                    false,
                )
            })
        } else if send_rotation {
            Some((
                (!present.is_empty()).then(|| {
                    encode_arc(
                        &EntityRotation {
                            id,
                            on_ground,
                            yaw,
                            pitch,
                        },
                        pkt_buf,
                    )
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
                if !present.is_empty() {
                    let pkt = encode_arc(
                        &SetEntityMotion {
                            id,
                            velocity: LpVec3([vel.x, vel.y, vel.z]),
                        },
                        pkt_buf,
                    );
                    broadcast_pkt(&pkt);
                }
            }
        }
        if let Some((packet, has_position, has_rotation)) = movement {
            if let Some(pkt) = packet {
                broadcast_pkt(&pkt);
            }
            if has_position {
                state.base = pos;
            }
            if has_rotation {
                state.last_yaw = yaw;
                state.last_pitch = pitch;
            }
        }
        if let Some(values) = data.metadata.pack_dirty()
            && !present.is_empty()
        {
            let pkt = encode_arc(&SetEntityData { id, values }, pkt_buf);
            broadcast_pkt(&pkt);
        }
        let dirty_equipment = data.equipment.take_dirty_wire_slots();
        if !dirty_equipment.is_empty() && !present.is_empty() {
            let pkt = encode_arc(
                &SetEquipment {
                    entity_id: VarInt(data.id),
                    slots: dirty_equipment,
                },
                pkt_buf,
            );
            broadcast_pkt(&pkt);
        }
        let head = pack_degrees(data.head_yaw);
        if head != state.last_head_yaw {
            if !present.is_empty() {
                let pkt = encode_arc(&EntityHeadRotation { id, yaw: head }, pkt_buf);
                broadcast_pkt(&pkt);
            }
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
        let bytes: Arc<[u8]> = encode(packet).into();
        for viewer in data.tracker.viewers() {
            sink(viewer, &bytes);
        }
    }
}

/// Zero-copy [`broadcast`] sending a shared `Arc<[u8]>` buffer.
pub fn broadcast_arc<P: Packet>(
    store: &EntityStore,
    key: crate::EntityKey,
    packet: &P,
    sink: &mut ArcSink<'_>,
) {
    if let Some(data) = store.data(key) {
        let bytes: Arc<[u8]> = encode(packet).into();
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
    fn a_skeleton_pairs_with_its_bow_in_the_main_hand() {
        let mut store = EntityStore::default();
        let skeleton = crate::mob::new_mob(
            crate::mob::MobKind::Skeleton,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(1),
        );
        let id = skeleton.data.id;
        store.insert(skeleton);
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let packet = packets
            .iter()
            .find(|p| p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("a skeleton pairs with its bow");
        let decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut packet.2.as_slice()).unwrap();
        assert_eq!(decoded.entity_id.0, id);
        assert_eq!(decoded.slots.len(), 1);
        assert_eq!(decoded.slots[0].0, crate::mob::MAIN_HAND);
        assert_eq!(
            decoded.slots[0].1.item,
            ferrite_item::item_id("minecraft:bow").unwrap()
        );
        assert_eq!(decoded.slots[0].1.count, 1);
    }

    #[test]
    fn any_mob_with_custom_six_slot_equipment_pairs_all_non_empty_slots() {
        let mut store = EntityStore::default();
        let zombie = crate::mob::new_mob(
            crate::mob::MobKind::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(2),
        );
        let id = zombie.data.id;
        let key = store.insert(zombie);
        let sword = ferrite_item::item_id("minecraft:iron_sword").unwrap();
        let helmet = ferrite_item::item_id("minecraft:diamond_helmet").unwrap();
        assert!(crate::mob::set_equipment(
            &mut store,
            key,
            crate::mob::MAIN_HAND as usize,
            sword
        ));
        assert!(crate::mob::set_equipment(&mut store, key, 5, helmet));
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let packet = packets
            .iter()
            .find(|p| p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("zombie pairs with custom equipment");
        let decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut packet.2.as_slice()).unwrap();
        assert_eq!(decoded.entity_id.0, id);
        assert_eq!(decoded.slots.len(), 2);
        assert!(
            decoded
                .slots
                .iter()
                .any(|(slot, s)| *slot == 0 && s.item == sword)
        );
        assert!(
            decoded
                .slots
                .iter()
                .any(|(slot, s)| *slot == 5 && s.item == helmet)
        );
    }

    #[test]
    fn a_vehicle_sends_its_passengers_when_a_viewer_starts_tracking() {
        let mut store = EntityStore::default();
        let mut vehicle = crate::mob::new_mob(
            crate::mob::MobKind::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
            &mut crate::Rng::new(1),
        );
        let vehicle_id = vehicle.data.id;
        vehicle.data.tracker.passengers = vec![77];
        store.insert(vehicle);
        let chunks = HashSet::from([(0, 0)]);
        let viewers = [viewer((Uuid::from_u128(1), 0), 8.0, 8.0, 10, &chunks)];
        let packets = tick(&mut store, &viewers);
        let packet = packets
            .iter()
            .find(|p| p.1 == SetPassengers::ID)
            .expect("a paired vehicle sends its passengers");
        let decoded = SetPassengers::decode(&mut packet.2.as_slice()).unwrap();
        assert_eq!(decoded.vehicle.0, vehicle_id);
        assert_eq!(
            decoded.passengers.iter().map(|id| id.0).collect::<Vec<_>>(),
            vec![77]
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

    #[test]
    fn equipment_change_broadcasts_set_equipment_and_late_viewers_receive_current_slots() {
        let mut store = EntityStore::default();
        let mut rng = crate::Rng::default();
        let zombie = crate::mob::new_mob(
            crate::mob::MobKind::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
            &mut rng,
        );
        let key = store.insert(zombie);
        let chunks = HashSet::from([(0, 0)]);
        let a = (Uuid::from_u128(1), 0);
        let b = (Uuid::from_u128(2), 0);

        // Initial tracking with empty equipment sends no SetEquipment.
        let initial = tick(&mut store, &[viewer(a, 8.0, 8.0, 10, &chunks)]);
        assert!(
            initial
                .iter()
                .all(|p| p.1 != SetEquipment::<ferrite_item::ItemStack>::ID)
        );

        // Equipping a helmet broadcasts SetEquipment to existing viewer `a`.
        let helmet = ferrite_item::ItemStack::from_name("minecraft:iron_helmet").unwrap();
        assert!(crate::mob::set_equipment_stack(
            &mut store,
            key,
            crate::EquipmentSlot::Head,
            Some(helmet.clone()),
        ));
        let updated = tick(&mut store, &[viewer(a, 8.0, 8.0, 10, &chunks)]);
        let eq_packet = updated
            .iter()
            .find(|p| p.0 == a && p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("SetEquipment broadcast on change");
        let decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut eq_packet.2.as_slice()).unwrap();
        assert_eq!(decoded.slots, vec![(5, helmet.clone())]);

        // A late viewer `b` starting to track receives the current helmet.
        let both = [
            viewer(a, 8.0, 8.0, 10, &chunks),
            viewer(b, 8.0, 8.0, 10, &chunks),
        ];
        let late = tick(&mut store, &both);
        let b_packet = late
            .iter()
            .find(|p| p.0 == b && p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("late viewer receives current equipment");
        let b_decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut b_packet.2.as_slice()).unwrap();
        assert_eq!(b_decoded.slots, vec![(5, helmet)]);

        // Clearing the helmet broadcasts ItemStack::empty() for slot 5.
        assert!(crate::mob::set_equipment_stack(
            &mut store,
            key,
            crate::EquipmentSlot::Head,
            None,
        ));
        let cleared = tick(&mut store, &both);
        let clear_packet = cleared
            .iter()
            .find(|p| p.0 == a && p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("SetEquipment broadcast on clear");
        let clear_decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut clear_packet.2.as_slice())
                .unwrap();
        assert_eq!(
            clear_decoded.slots,
            vec![(5, ferrite_item::ItemStack::empty())]
        );
    }

    #[test]
    fn per_type_metadata_reaches_existing_and_late_viewers() {
        let mut store = EntityStore::default();
        let mut rng = crate::Rng::new(7);
        let dragon = crate::mob::new_mob(
            crate::mob::MobKind::EnderDragon,
            Vec3::new(8.0, 80.0, 8.0),
            &mut rng,
        );
        let dragon_id = dragon.data.id;
        let dragon_key = store.insert(dragon);

        let mut zombie = crate::mob::new_mob(
            crate::mob::MobKind::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
            &mut rng,
        );
        zombie.data.mob.as_mut().unwrap().age = -24_000;
        let zombie_id = zombie.data.id;
        let zombie_key = store.insert(zombie);

        let mut parrot = crate::mob::new_mob(
            crate::mob::MobKind::Parrot,
            Vec3::new(8.0, 64.0, 8.0),
            &mut rng,
        );
        parrot.data.mob.as_mut().unwrap().variant = Some(2);
        let parrot_id = parrot.data.id;
        store.insert(parrot);

        let chunks = HashSet::from([(0, 0)]);
        let a = (Uuid::from_u128(1), 0);
        let b = (Uuid::from_u128(2), 0);

        let initial = tick(&mut store, &[viewer(a, 8.0, 64.0, 10, &chunks)]);
        let zombie_meta = initial
            .iter()
            .filter_map(|p| {
                if p.0 == a && p.1 == SetEntityData::ID {
                    SetEntityData::decode(&mut p.2.as_slice()).ok()
                } else {
                    None
                }
            })
            .find(|pkt| pkt.id.0 == zombie_id)
            .expect("baby zombie metadata sent on initial pairing");
        assert!(zombie_meta.values.iter().any(|e| {
            e.index == crate::mob::ZOMBIE_BABY_INDEX
                && e.value == ferrite_protocol::packets::play::MetaValue::Boolean(true)
        }));
        let parrot_meta = initial
            .iter()
            .filter_map(|p| {
                if p.0 == a && p.1 == SetEntityData::ID {
                    SetEntityData::decode(&mut p.2.as_slice()).ok()
                } else {
                    None
                }
            })
            .find(|pkt| pkt.id.0 == parrot_id)
            .expect("parrot variant metadata sent on initial pairing");
        assert!(parrot_meta.values.iter().any(|e| {
            e.index == crate::tame::PARROT_VARIANT_INDEX
                && e.value == ferrite_protocol::packets::play::MetaValue::Int(2)
        }));

        // Mutate dragon phase and zombie drowned-conversion state; existing viewer `a` receives updates.
        store
            .data_mut(dragon_key)
            .unwrap()
            .mob
            .as_mut()
            .unwrap()
            .dragon_phase = crate::dragon::DragonPhase::PerchFlaming;
        store
            .data_mut(zombie_key)
            .unwrap()
            .mob
            .as_mut()
            .unwrap()
            .in_water_time = crate::ai::DROWNED_CONVERSION_TICKS;

        let updated = tick(&mut store, &[viewer(a, 8.0, 64.0, 10, &chunks)]);
        let dragon_update = updated
            .iter()
            .filter_map(|p| {
                if p.0 == a && p.1 == SetEntityData::ID {
                    SetEntityData::decode(&mut p.2.as_slice()).ok()
                } else {
                    None
                }
            })
            .find(|pkt| pkt.id.0 == dragon_id)
            .expect("dragon phase update broadcast to existing viewer");
        assert!(dragon_update.values.iter().any(|e| {
            e.index == crate::dragon::PHASE_INDEX
                && e.value == ferrite_protocol::packets::play::MetaValue::Int(5)
        }));
        let zombie_update = updated
            .iter()
            .filter_map(|p| {
                if p.0 == a && p.1 == SetEntityData::ID {
                    SetEntityData::decode(&mut p.2.as_slice()).ok()
                } else {
                    None
                }
            })
            .find(|pkt| pkt.id.0 == zombie_id)
            .expect("zombie drowned conversion update broadcast");
        assert!(zombie_update.values.iter().any(|e| {
            e.index == crate::mob::ZOMBIE_DROWNED_CONVERSION_INDEX
                && e.value == ferrite_protocol::packets::play::MetaValue::Boolean(true)
        }));

        // Late viewer `b` receives current dragon phase and zombie flags on pairing.
        let late = tick(
            &mut store,
            &[
                viewer(a, 8.0, 64.0, 10, &chunks),
                viewer(b, 8.0, 64.0, 10, &chunks),
            ],
        );
        let late_dragon = late
            .iter()
            .filter_map(|p| {
                if p.0 == b && p.1 == SetEntityData::ID {
                    SetEntityData::decode(&mut p.2.as_slice()).ok()
                } else {
                    None
                }
            })
            .find(|pkt| pkt.id.0 == dragon_id)
            .expect("late viewer receives dragon phase");
        assert!(late_dragon.values.iter().any(|e| {
            e.index == crate::dragon::PHASE_INDEX
                && e.value == ferrite_protocol::packets::play::MetaValue::Int(5)
        }));
    }

    #[test]
    fn body_and_saddle_equipment_pairs_and_broadcasts_updates() {
        let mut store = EntityStore::default();
        let mut rng = crate::Rng::new(9);
        let horse = crate::mob::new_mob(
            crate::mob::MobKind::Horse,
            Vec3::new(8.0, 64.0, 8.0),
            &mut rng,
        );
        let horse_id = horse.data.id;
        let key = store.insert(horse);
        let chunks = HashSet::from([(0, 0)]);
        let a = (Uuid::from_u128(1), 0);
        let b = (Uuid::from_u128(2), 0);

        let _ = tick(&mut store, &[viewer(a, 8.0, 64.0, 10, &chunks)]);

        let armor = ferrite_item::ItemStack::from_name("minecraft:diamond_horse_armor").unwrap();
        let saddle = ferrite_item::ItemStack::from_name("minecraft:saddle").unwrap();
        assert!(crate::mob::set_equipment_stack(
            &mut store,
            key,
            crate::EquipmentSlot::Body,
            Some(armor.clone()),
        ));
        assert!(crate::mob::set_equipment_stack(
            &mut store,
            key,
            crate::EquipmentSlot::Saddle,
            Some(saddle.clone()),
        ));
        let mob_state = crate::mob::state(&store, key).unwrap();
        assert!(mob_state.saddled);
        assert_eq!(mob_state.armor, armor.item);

        let updated = tick(&mut store, &[viewer(a, 8.0, 64.0, 10, &chunks)]);
        let pkt = updated
            .iter()
            .find(|p| p.0 == a && p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("SetEquipment broadcast for body and saddle");
        let decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut pkt.2.as_slice()).unwrap();
        assert_eq!(decoded.entity_id.0, horse_id);
        assert_eq!(decoded.slots, vec![(6, armor.clone()), (7, saddle.clone())]);

        let late = tick(
            &mut store,
            &[
                viewer(a, 8.0, 64.0, 10, &chunks),
                viewer(b, 8.0, 64.0, 10, &chunks),
            ],
        );
        let late_pkt = late
            .iter()
            .find(|p| p.0 == b && p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
            .expect("late viewer receives body and saddle");
        let late_decoded =
            SetEquipment::<ferrite_item::ItemStack>::decode(&mut late_pkt.2.as_slice()).unwrap();
        assert_eq!(late_decoded.slots, vec![(6, armor), (7, saddle)]);
    }

    #[test]
    fn new_mob_metadata_wolf_body_and_pig_saddle_pair_on_start() {
        let mut store = EntityStore::default();
        let mut rng = crate::Rng::new(11);
        let chunks = HashSet::from([(0, 0)]);
        let viewer_key = (Uuid::from_u128(99), 0);

        // Wolf with wolf_armor in BODY slot.
        let wolf = crate::mob::new_mob(
            crate::mob::MobKind::Wolf,
            Vec3::new(4.0, 64.0, 4.0),
            &mut rng,
        );
        let wolf_id = wolf.data.id;
        let wolf_key = store.insert(wolf);
        let wolf_armor = ferrite_item::ItemStack::from_name("minecraft:wolf_armor").unwrap();
        assert!(crate::mob::set_equipment_stack(
            &mut store,
            wolf_key,
            crate::EquipmentSlot::Body,
            Some(wolf_armor.clone()),
        ));

        // Saddled Pig (SADDLE slot).
        let pig = crate::mob::new_mob(
            crate::mob::MobKind::Pig,
            Vec3::new(6.0, 64.0, 6.0),
            &mut rng,
        );
        let pig_id = pig.data.id;
        let pig_key = store.insert(pig);
        assert!(crate::pig::set_saddled(&mut store, pig_key, true));

        // Sheared SnowGolem (index 16 byte 0 != default 0x10).
        let mut golem = crate::mob::new_mob(
            crate::mob::MobKind::SnowGolem,
            Vec3::new(8.0, 64.0, 8.0),
            &mut rng,
        );
        golem.data.mob.as_mut().unwrap().sheared = true;
        let golem_id = golem.data.id;
        store.insert(golem);

        // Warm Frog (index 18 FrogVariant(2) != default 1).
        let mut frog = crate::mob::new_mob(
            crate::mob::MobKind::Frog,
            Vec3::new(10.0, 64.0, 10.0),
            &mut rng,
        );
        frog.data.mob.as_mut().unwrap().variant = Some(2);
        let frog_id = frog.data.id;
        store.insert(frog);

        // Baby Piglin (index 17 Boolean(true)).
        let mut piglin = crate::mob::new_mob(
            crate::mob::MobKind::Piglin,
            Vec3::new(12.0, 64.0, 12.0),
            &mut rng,
        );
        piglin.data.mob.as_mut().unwrap().age = -24000;
        let piglin_id = piglin.data.id;
        store.insert(piglin);

        let sent = tick(&mut store, &[viewer(viewer_key, 8.0, 64.0, 10, &chunks)]);

        let eq_for = |entity_id: i32| {
            sent.iter()
                .filter(|p| p.1 == SetEquipment::<ferrite_item::ItemStack>::ID)
                .filter_map(|p| {
                    SetEquipment::<ferrite_item::ItemStack>::decode(&mut p.2.as_slice()).ok()
                })
                .find(|pkt| pkt.entity_id.0 == entity_id)
                .map(|pkt| pkt.slots)
                .unwrap_or_default()
        };
        let meta_for = |entity_id: i32| {
            sent.iter()
                .filter(|p| p.1 == SetEntityData::ID)
                .filter_map(|p| SetEntityData::decode(&mut p.2.as_slice()).ok())
                .find(|pkt| pkt.id.0 == entity_id)
                .map(|pkt| pkt.values)
                .unwrap_or_default()
        };

        assert_eq!(eq_for(wolf_id), vec![(6, wolf_armor)]);
        assert_eq!(
            eq_for(pig_id),
            vec![(
                7,
                ferrite_item::ItemStack::from_name("minecraft:saddle").unwrap()
            )]
        );
        assert!(meta_for(golem_id).iter().any(|e| {
            e.index == crate::metadata::snow_golem::PUMPKIN && e.value == MetaValue::Byte(0)
        }));
        assert!(meta_for(frog_id).iter().any(|e| {
            e.index == crate::metadata::frog::VARIANT && e.value == MetaValue::FrogVariant(2)
        }));
        assert!(meta_for(piglin_id).iter().any(|e| {
            e.index == crate::metadata::piglin::BABY && e.value == MetaValue::Boolean(true)
        }));
    }
}
