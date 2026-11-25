use super::fx::FxMap;
use super::{Dimension, WorldId, chat::PlayerChatState};
use ferrite_net::game::{Outbound, OutboundSender, packet, shared_packet};
use ferrite_profiler::Timings;
use ferrite_protocol::{
    FrameEncoder, VarInt,
    packets::{login::Property, play::*},
    types::PlainText,
};
use std::collections::hash_map::Entry;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use uuid::Uuid;
/// What an observer was last told about a target: just the pose, so tracking does not copy a
/// whole player (name, skin properties, channel handle) per observed pair per tick.
#[derive(Clone, Copy, PartialEq)]
struct Pose {
    pos: [f64; 3],
    yaw: f32,
    pitch: f32,
}

/// What an observer last saw, plus the tick-input slot that target occupied. The slot is only a
/// hint: it is validated against the UUID before use, so a reordered or replaced player falls
/// back to the UUID index instead of tracking the wrong entity.
#[derive(Clone, Copy)]
struct SeenTarget {
    pose: Pose,
    slot: u32,
}

/// Minimal per-player snapshot copied for parallel entity tracking ticks.
#[derive(Clone)]
struct TickPlayer {
    uuid: Uuid,
    entity: i32,
    pos: [f64; 3],
    yaw: f32,
    pitch: f32,
    ground: bool,
    dimension: Dimension,
    world_id: WorldId,
    outbound: OutboundSender,
}

impl From<&GlobalPlayer> for TickPlayer {
    fn from(p: &GlobalPlayer) -> Self {
        Self {
            uuid: p.uuid,
            entity: p.entity,
            pos: p.pos,
            yaw: p.yaw,
            pitch: p.pitch,
            ground: p.ground,
            dimension: p.dimension,
            world_id: p.world_id,
            outbound: p.outbound.clone(),
        }
    }
}

impl From<&TickPlayer> for Pose {
    fn from(p: &TickPlayer) -> Self {
        Self {
            pos: p.pos,
            yaw: p.yaw,
            pitch: p.pitch,
        }
    }
}

impl From<&GlobalPlayer> for Pose {
    fn from(p: &GlobalPlayer) -> Self {
        Self {
            pos: p.pos,
            yaw: p.yaw,
            pitch: p.pitch,
        }
    }
}

/// Pose and world identity of a player at the last tracking pass. Entity tracking only changes
/// when one of these changes: the visible set is a function of the player set and the poses.
#[derive(Clone, Copy, PartialEq)]
struct TrackedPose {
    pose: Pose,
    world_id: WorldId,
    dimension: Dimension,
}

impl From<&GlobalPlayer> for TrackedPose {
    fn from(p: &GlobalPlayer) -> Self {
        Self {
            pose: Pose::from(p),
            world_id: p.world_id,
            dimension: p.dimension,
        }
    }
}

impl From<&TickPlayer> for TrackedPose {
    fn from(p: &TickPlayer) -> Self {
        Self {
            pose: Pose::from(p),
            world_id: p.world_id,
            dimension: p.dimension,
        }
    }
}

#[derive(Clone)]
pub struct GlobalPlayer {
    pub uuid: Uuid,
    pub entity: i32,
    pub name: String,
    pub properties: Vec<Property>,
    pub game_mode: i32,
    pub outbound: OutboundSender,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub ground: bool,
    /// The dimension the player is in; entity tracking never crosses dimensions.
    pub dimension: Dimension,
    /// Stable world identity; two worlds can share the same dimension type.
    pub world_id: WorldId,
    /// Client `chat_mode`: 0 shown, 1 commands only, 2 hidden.
    pub chat_visibility: u8,
    /// Registry id of `minecraft:chat` used to decorate broadcast messages.
    pub chat_type: i32,
    pub chat: PlayerChatState,
}
impl GlobalPlayer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uuid: Uuid,
        entity: i32,
        name: String,
        properties: Vec<Property>,
        outbound: OutboundSender,
        secure_chat: bool,
        chat_type: i32,
        chat_visibility: u8,
    ) -> Self {
        Self {
            uuid,
            entity,
            name,
            properties,
            game_mode: 0,
            outbound,
            pos: [0.5, 65.0, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            ground: true,
            dimension: Dimension::Overworld,
            world_id: WorldId::Vanilla(Dimension::Overworld),
            chat_visibility,
            chat_type,
            chat: PlayerChatState::new(secure_chat),
        }
    }
    fn info(&self) -> PlayerInfo {
        PlayerInfo {
            uuid: self.uuid,
            name: self.name.clone(),
            properties: self.properties.clone(),
            chat_session: self.chat.session_data().cloned(),
            game_mode: VarInt(self.game_mode),
        }
    }
}
fn tell(player: &GlobalPlayer, text: &str) {
    send(
        player,
        &SystemChat {
            content: PlainText(text.into()),
            overlay: false,
        },
    );
}
fn kick(player: &GlobalPlayer, reason: &str) {
    let _ = player.outbound.send(Outbound::Disconnect(reason.into()));
}
/// Vanilla `StringUtil.isAllowedChatCharacter`: control characters and `§` are illegal.
fn is_illegal_chat(message: &str) -> bool {
    message
        .chars()
        .any(|c| c == '\u{a7}' || (c as u32) < 0x20 || c as u32 == 0x7f)
}
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
#[derive(Default)]
struct TickInput {
    players: Vec<TickPlayer>,
    grid: FxMap<(WorldId, i32, i32), Vec<usize>>,
    grid_pool: Vec<Vec<usize>>,
    index: FxMap<Uuid, usize>,
    /// Observers whose pose or world changed since the previous pass; only they need a full
    /// candidate scan. Other observers only inspect [`Self::moved_targets`].
    observer_dirty: Vec<bool>,
    /// Target slots whose pose or world changed. A static observer only has to reconsider
    /// these targets, not its entire 3x3 grid neighbourhood.
    moved_targets: Vec<usize>,
    /// The player count changed, so the old visible maps cannot be updated incrementally.
    full_scan: bool,
}

struct MoveCache {
    old: Pose,
    new: Pose,
    entity: i32,
    packets: Vec<Outbound>,
}

#[derive(Default)]
struct WorkerScratch {
    spawns: FxMap<Uuid, Outbound>,
    /// Movement packets for one target, indexed by its tick-input slot. All in-range observers
    /// normally saw the same previous pose, so one encode serves the whole fan-out without a
    /// 56-byte hash key per observer-target pair.
    moves: Vec<Option<MoveCache>>,
    candidates: Vec<usize>,
    /// Per-observer generation stamp for candidate lookup; replaces a sort + dedup of every
    /// observer's 3x3 grid neighbourhood on each tracking tick.
    candidate_stamp: Vec<u32>,
    candidate_epoch: u32,
}

struct TrackingJob {
    seq: u64,
    input: Arc<TickInput>,
    shared_encoder: Option<Arc<FrameEncoder>>,
    pending_chunks: Vec<(usize, Vec<FxMap<Uuid, SeenTarget>>)>,
    done_chunks: Vec<(usize, Vec<FxMap<Uuid, SeenTarget>>)>,
    active_workers: usize,
    stopping: bool,
}

struct TrackingWorkers {
    state: Arc<(Mutex<TrackingJob>, Condvar, Condvar)>,
    handles: Vec<JoinHandle<()>>,
}

impl TrackingWorkers {
    fn new(threads: usize) -> Self {
        let state = Arc::new((
            Mutex::new(TrackingJob {
                seq: 0,
                input: Arc::new(TickInput::default()),
                shared_encoder: None,
                pending_chunks: Vec::new(),
                done_chunks: Vec::new(),
                active_workers: 0,
                stopping: false,
            }),
            Condvar::new(),
            Condvar::new(),
        ));
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let s = Arc::clone(&state);
            handles.push(std::thread::spawn(move || {
                let (lock, work_cv, done_cv) = &*s;
                let mut scratch = WorkerScratch::default();
                loop {
                    let (first, mut chunk, input, shared_encoder) = {
                        let mut job = lock.lock().unwrap();
                        loop {
                            if let Some((first, chunk)) = job.pending_chunks.pop() {
                                job.active_workers += 1;
                                break (
                                    first,
                                    chunk,
                                    Arc::clone(&job.input),
                                    job.shared_encoder.clone(),
                                );
                            }
                            if job.stopping {
                                return;
                            }
                            job = work_cv.wait(job).unwrap();
                        }
                    };
                    run_tracking_chunk(
                        &input,
                        shared_encoder.as_deref(),
                        first,
                        &mut chunk,
                        &mut scratch,
                    );
                    drop(input);
                    drop(shared_encoder);
                    let mut job = lock.lock().unwrap();
                    job.done_chunks.push((first, chunk));
                    job.active_workers -= 1;
                    if job.pending_chunks.is_empty() && job.active_workers == 0 {
                        done_cv.notify_one();
                    }
                }
            }));
        }
        Self { state, handles }
    }
}

impl Drop for TrackingWorkers {
    fn drop(&mut self) {
        {
            let (lock, work_cv, _) = &*self.state;
            let mut job = lock.lock().unwrap();
            job.stopping = true;
            work_cv.notify_all();
        }
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

fn cell_of(p: &TickPlayer) -> (WorldId, i32, i32) {
    const CELL: f64 = 48.0;
    (
        p.world_id,
        (p.pos[0] / CELL).floor() as i32,
        (p.pos[2] / CELL).floor() as i32,
    )
}

fn run_tracking_chunk(
    input: &TickInput,
    shared_encoder: Option<&FrameEncoder>,
    first: usize,
    chunk: &mut [FxMap<Uuid, SeenTarget>],
    scratch: &mut WorkerScratch,
) {
    scratch.spawns.clear();
    // Keep movement packets across chunks and ticks. The cache records the exact old/new pose
    // and entity id, so a stale slot can never be sent for a different player.
    scratch.moves.truncate(input.players.len());
    scratch.moves.resize_with(input.players.len(), || None);
    for (offset, seen) in chunk.iter_mut().enumerate() {
        let observer_index = first + offset;
        let observer = &input.players[observer_index];
        let (world_id, cx, cz) = cell_of(observer);
        let observer_dirty = input.full_scan || input.observer_dirty[observer_index];
        if !observer_dirty && input.moved_targets.is_empty() {
            continue;
        }
        if observer_dirty || input.moved_targets.len() >= 16 {
            // A generation stamp marks each player once, so candidates from the previously seen
            // set and from up to nine overlapping grid cells need no sort or dedup.
            let only_moved = !observer_dirty;
            let epoch = scratch.candidate_epoch.wrapping_add(1);
            if epoch == 0 {
                scratch.candidate_stamp.fill(0);
            }
            scratch.candidate_epoch = if epoch == 0 { 1 } else { epoch };
            let epoch = scratch.candidate_epoch;
            scratch.candidate_stamp.resize(input.players.len(), 0);
            scratch.candidates.clear();
            {
                let stamps = &mut scratch.candidate_stamp;
                let candidates = &mut scratch.candidates;
                for (target_uuid, tracked) in seen.iter() {
                    let slot = tracked.slot as usize;
                    let ti = if input
                        .players
                        .get(slot)
                        .is_some_and(|p| p.uuid == *target_uuid)
                    {
                        slot
                    } else if let Some(&ti) = input.index.get(target_uuid) {
                        ti
                    } else {
                        continue;
                    };
                    if (!only_moved || input.observer_dirty[ti]) && stamps[ti] != epoch {
                        stamps[ti] = epoch;
                        candidates.push(ti);
                    }
                }
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(near) = input.grid.get(&(world_id, cx + dx, cz + dz)) {
                            for &ti in near {
                                if (!only_moved || input.observer_dirty[ti]) && stamps[ti] != epoch
                                {
                                    stamps[ti] = epoch;
                                    candidates.push(ti);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Small moved set: scanning `moved_targets` directly beats 9 spatial-cell lookups.
            scratch.candidates.clear();
            scratch.candidates.extend_from_slice(&input.moved_targets);
        }
        for &ti in &scratch.candidates {
            let target = &input.players[ti];
            if observer.uuid == target.uuid {
                continue;
            }
            // Vanilla `ChunkMap.TrackedEntity` only tracks entities in the same level.
            let inside = observer.world_id == target.world_id && {
                let dx = observer.pos[0] - target.pos[0];
                let dy = observer.pos[1] - target.pos[1];
                let dz = observer.pos[2] - target.pos[2];
                dx * dx + dy * dy + dz * dz <= 48.0 * 48.0
            };
            match seen.entry(target.uuid) {
                Entry::Vacant(entry) => {
                    if inside {
                        let spawn_pkt = scratch.spawns.entry(target.uuid).or_insert_with(|| {
                            encode_for_fanout(
                                shared_encoder,
                                &AddEntity {
                                    id: VarInt(target.entity),
                                    uuid: target.uuid,
                                    entity_type: VarInt(159),
                                    x: target.pos[0],
                                    y: target.pos[1],
                                    z: target.pos[2],
                                    velocity: ferrite_protocol::types::LpVec3::ZERO,
                                    pitch: angle(target.pitch),
                                    yaw: angle(target.yaw),
                                    head_yaw: angle(target.yaw),
                                    data: VarInt(0),
                                },
                            )
                        });
                        let _ = observer.outbound.send(spawn_pkt.clone());
                        entry.insert(SeenTarget {
                            pose: Pose::from(target),
                            slot: ti as u32,
                        });
                    }
                }
                Entry::Occupied(mut entry) => {
                    if !inside {
                        let _ = observer.outbound.send(packet(&RemoveEntities {
                            ids: vec![VarInt(target.entity)],
                        }));
                        entry.remove();
                        continue;
                    }
                    let old = *entry.get();
                    // Bots and idle players keep the same pose across many ticks. Do not hash a
                    // movement cache key or allocate a packet list when there is nothing to send.
                    if old.pose.pos != target.pos
                        || old.pose.yaw != target.yaw
                        || old.pose.pitch != target.pitch
                    {
                        // Observers that last saw the target in the same pose get identical
                        // packets, so encode them once per tick. Reusing the packet buffer also
                        // keeps walking targets allocation-free after the first chunk.
                        let target_pose = Pose::from(target);
                        let cache = &mut scratch.moves[ti];
                        if cache.as_ref().is_none_or(|cache| {
                            cache.old != old.pose
                                || cache.new != target_pose
                                || cache.entity != target.entity
                        }) {
                            let cache = cache.get_or_insert_with(|| MoveCache {
                                old: old.pose,
                                new: target_pose,
                                entity: target.entity,
                                packets: Vec::new(),
                            });
                            cache.old = old.pose;
                            cache.new = target_pose;
                            cache.entity = target.entity;
                            movement_packets(&mut cache.packets, shared_encoder, target, &old.pose);
                        }
                        if let Some(cache) = &*cache {
                            for packet in &cache.packets {
                                let _ = observer.outbound.send(packet.clone());
                            }
                        }
                        let tracked = entry.get_mut();
                        tracked.pose = target_pose;
                        tracked.slot = ti as u32;
                    } else if old.slot != ti as u32 {
                        entry.get_mut().slot = ti as u32;
                    }
                }
            }
        }
    }
}

pub struct GlobalRegion {
    players: FxMap<Uuid, GlobalPlayer>,
    /// Per observer, the targets it was told about and the pose it last saw them in.
    visible: FxMap<Uuid, FxMap<Uuid, SeenTarget>>,
    /// Scope tree attached by the game loop; `None` until the first tick.
    pub timings: Option<Arc<Timings>>,
    /// Shared encoder matching server compression config so multi-viewer movement and chat
    /// broadcasts compress wire frames once per message instead of once per recipient.
    shared_encoder: Option<Arc<FrameEncoder>>,
    tick_input: Arc<TickInput>,
    single_scratch: WorkerScratch,
    observed_buf: Vec<FxMap<Uuid, SeenTarget>>,
    workers: Option<TrackingWorkers>,
    /// Poses and world identities seen by the last tracking pass. A tick with no change to any
    /// of them cannot add, remove or move a tracked target, so it can skip the 800x800 scan
    /// entirely while keeping the previous visible sets.
    last_poses: FxMap<Uuid, TrackedPose>,
    #[cfg(test)]
    tracking_passes: u64,
}
impl Default for GlobalRegion {
    fn default() -> Self {
        Self {
            players: FxMap::default(),
            visible: FxMap::default(),
            timings: None,
            shared_encoder: None,
            tick_input: Arc::new(TickInput::default()),
            single_scratch: WorkerScratch::default(),
            observed_buf: Vec::new(),
            workers: None,
            last_poses: FxMap::default(),
            #[cfg(test)]
            tracking_passes: 0,
        }
    }
}
impl GlobalRegion {
    /// Configures pre-compressed fan-out encoding matching `ServerConfig`.
    pub fn set_shared_compression(
        &mut self,
        threshold: Option<usize>,
        level: u32,
        skip_incompressible: bool,
    ) {
        self.shared_encoder = Some(Arc::new(FrameEncoder::new_compressed(
            threshold,
            level,
            skip_incompressible,
        )));
    }
}
fn encode_for_fanout<P: ferrite_protocol::Packet>(
    encoder: Option<&FrameEncoder>,
    p: &P,
) -> Outbound {
    match encoder {
        Some(enc) => shared_packet(enc, p),
        None => packet(p),
    }
}
fn send<P: ferrite_protocol::Packet>(to: &GlobalPlayer, p: &P) {
    let _ = to.outbound.send(packet(p));
}
fn angle(v: f32) -> u8 {
    (v * 256.0 / 360.0).floor() as i32 as u8
}
/// The packets that move `old` to `target`'s pose: a relative move (with rotation if it turned),
/// a teleport when the step does not fit, a rotation, and the head rotation when it turned.
fn movement_packets(
    out: &mut Vec<Outbound>,
    encoder: Option<&FrameEncoder>,
    target: &TickPlayer,
    old: &Pose,
) {
    out.clear();
    let delta = [
        (target.pos[0] * 4096.0).floor() as i64 - (old.pos[0] * 4096.0).floor() as i64,
        (target.pos[1] * 4096.0).floor() as i64 - (old.pos[1] * 4096.0).floor() as i64,
        (target.pos[2] * 4096.0).floor() as i64 - (old.pos[2] * 4096.0).floor() as i64,
    ];
    let moved = delta.iter().any(|v| *v != 0);
    let rotated = old.yaw != target.yaw || old.pitch != target.pitch;
    if moved {
        if delta.iter().all(|d| i16::try_from(*d).is_ok()) {
            let (dx, dy, dz) = (delta[0] as i16, delta[1] as i16, delta[2] as i16);
            let properties = VarInt(i32::from(target.ground));
            if rotated {
                out.push(encode_for_fanout(
                    encoder,
                    &EntityMoveRotation {
                        id: VarInt(target.entity),
                        properties,
                        dx,
                        dy,
                        dz,
                        yaw: angle(target.yaw),
                        pitch: angle(target.pitch),
                    },
                ));
            } else {
                out.push(encode_for_fanout(
                    encoder,
                    &EntityMove {
                        id: VarInt(target.entity),
                        properties,
                        dx,
                        dy,
                        dz,
                    },
                ));
            }
        } else {
            out.push(encode_for_fanout(
                encoder,
                &EntityTeleport {
                    id: VarInt(target.entity),
                    path_type: VarInt(0),
                    x: target.pos[0],
                    y: target.pos[1],
                    z: target.pos[2],
                    yaw: target.yaw,
                    pitch: target.pitch,
                    on_ground: target.ground,
                },
            ));
        }
    } else if rotated {
        out.push(encode_for_fanout(
            encoder,
            &EntityRotation {
                id: VarInt(target.entity),
                on_ground: target.ground,
                yaw: angle(target.yaw),
                pitch: angle(target.pitch),
            },
        ));
    }
    if rotated {
        out.push(encode_for_fanout(
            encoder,
            &EntityHeadRotation {
                id: VarInt(target.entity),
                yaw: angle(target.yaw),
            },
        ));
    }
}

impl GlobalRegion {
    pub fn join(&mut self, p: GlobalPlayer) {
        let joined = encode_for_fanout(self.shared_encoder.as_deref(), &p.info());
        let mut all_entries = Vec::with_capacity(self.players.len() + 1);
        for old in self.players.values() {
            let _ = old.outbound.send(joined.clone());
            all_entries.push(old.info());
        }
        all_entries.push(p.info());
        let _ = p.outbound.send(packet(&PlayerInfoBatch(all_entries)));
        self.broadcast(&format!("{} joined the game", p.name));
        self.players.insert(p.uuid, p);
    }
    /// Update both the dimension rules and the world's independent identity.
    pub fn set_world(&mut self, id: Uuid, dimension: Dimension, world_id: WorldId) {
        if let Some(player) = self.players.get_mut(&id) {
            player.dimension = dimension;
            player.world_id = world_id;
        }
    }
    pub fn update(&mut self, id: Uuid, pos: [f64; 3], yaw: f32, pitch: f32, ground: bool) {
        if let Some(p) = self.players.get_mut(&id) {
            p.pos = pos;
            p.yaw = yaw;
            p.pitch = pitch;
            p.ground = ground;
        }
    }
    pub fn set_mode(&mut self, id: Uuid, mode: i32) {
        if let Some(player) = self.players.get_mut(&id) {
            player.game_mode = mode;
        }
        let update = packet(&PlayerGameMode {
            uuid: id,
            game_mode: VarInt(mode),
        });
        for player in self.players.values() {
            let _ = player.outbound.send(update.clone());
        }
    }
    pub fn broadcast_text(&self, message: &str) {
        self.broadcast(message);
    }
    /// Registers or replaces a player's chat session and tells every client about it, so they
    /// can verify that player's message signatures (`INITIALIZE_CHAT`).
    pub fn chat_session(&mut self, id: Uuid, data: RemoteChatSessionData) {
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        match player.chat.update_session(id, data.clone()) {
            Ok(true) => {
                let update = PlayerInfoChatSession {
                    uuid: id,
                    chat_session: Some(data),
                };
                for other in self.players.values() {
                    send(other, &update);
                }
            }
            Ok(false) => {}
            Err(error) => kick(player, error.disconnect_reason()),
        }
    }
    /// Applies a client's acknowledgement offset (`chat_ack`).
    pub fn chat_ack(&mut self, id: Uuid, offset: i32) {
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        if let Err(error) = player.chat.apply_ack(offset) {
            tracing::error!(%id, %error, "invalid chat acknowledgement offset");
            kick(player, "Chat message validation failed");
        }
    }
    /// Applies the last-seen update carried by a signed chat command. The command's own signed
    /// arguments are not verified yet, but the acknowledgement state must stay in sync.
    pub fn apply_chat_update(&mut self, id: Uuid, update: &LastSeenUpdate) {
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        if let Err(error) = player.chat.unpack_last_seen(update) {
            tracing::error!(%id, %error, "invalid chat last-seen update");
            kick(player, "Chat message validation failed");
        }
    }
    /// Validates a `serverbound_chat` message and broadcasts it as `player_chat`.
    pub fn chat(&mut self, id: Uuid, packet: &ServerboundChat) {
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        // Vanilla `tryHandleChat`: hidden chat is rejected before the signature is even checked.
        if player.chat_visibility == 2 {
            tell(player, "Chat is disabled in your client settings");
            return;
        }
        if is_illegal_chat(&packet.message) {
            kick(player, "Illegal characters in chat");
            return;
        }
        let last_seen = match player.chat.unpack_last_seen(&packet.last_seen) {
            Ok(last_seen) => last_seen,
            Err(error) => {
                tracing::error!(%id, %error, "invalid chat last-seen update");
                kick(player, "Chat message validation failed");
                return;
            }
        };
        let message = match player.chat.decode(packet, last_seen, now_millis()) {
            Ok(message) => message,
            Err(error) => {
                tell(player, error.message());
                return;
            }
        };
        let name = player.name.clone();
        let chat_type = player.chat_type;
        let bound = ChatTypeBound {
            chat_type: VarInt(chat_type),
            name: PlainText(name),
            target_name: None,
        };
        for recipient in self.players.values_mut() {
            // Vanilla `ServerPlayer.acceptsChatMessages`: only clients with chat "Shown" receive
            // player chat, and a skipped client must not advance the acknowledgement index.
            if recipient.chat_visibility != 0 {
                continue;
            }
            let global_index = recipient.chat.next_global_index();
            send(
                recipient,
                &ClientboundPlayerChat {
                    global_index: VarInt(global_index),
                    sender: id,
                    index: VarInt(message.index),
                    signature: message.signature,
                    body: SignedMessageBody {
                        content: message.content.clone(),
                        timestamp: message.timestamp,
                        salt: message.salt,
                        last_seen: message.last_seen.clone(),
                    },
                    unsigned_content: None,
                    filter_mask: VarInt(0),
                    chat_type: bound.clone(),
                },
            );
            if let Some(signature) = message.signature {
                recipient.chat.add_pending(signature);
            }
        }
    }
    fn broadcast(&self, text: &str) {
        // Compress wire frame once and share across all online players.
        let message = encode_for_fanout(
            self.shared_encoder.as_deref(),
            &SystemChat {
                content: PlainText(text.into()),
                overlay: false,
            },
        );
        for p in self.players.values() {
            let _ = p.outbound.send(message.clone());
        }
    }
    pub fn leave(&mut self, id: Uuid) {
        if let Some(p) = self.players.remove(&id) {
            for other in self.players.values() {
                send(other, &PlayerInfoRemove { uuids: vec![id] });
                if self
                    .visible
                    .get_mut(&other.uuid)
                    .is_some_and(|seen| seen.remove(&id).is_some())
                {
                    send(
                        other,
                        &RemoveEntities {
                            ids: vec![VarInt(p.entity)],
                        },
                    );
                }
            }
            self.visible.remove(&id);
            self.broadcast(&format!("{} left the game", p.name));
        }
    }
    pub fn shutdown(&mut self) {
        for p in self.players.values() {
            let _ = p
                .outbound
                .send(Outbound::Disconnect("Server shutting down".into()));
        }
    }
    /// Whether any tracked input changed since the last pass. A different length catches a pure
    /// join or leave; the per-player comparison also catches a same-size replacement and direct
    /// position/world edits made outside [`GlobalRegion::update`].
    fn tracking_changed(&self) -> bool {
        if self.players.len() != self.last_poses.len() {
            return true;
        }
        self.players.values().any(|p| {
            self.last_poses
                .get(&p.uuid)
                .is_none_or(|last| *last != TrackedPose::from(p))
        })
    }

    /// Replaces the remembered pose set with the live players. The map keeps its capacity, so
    /// this is allocation-free after the first full pass.
    fn remember_poses(&mut self) {
        self.last_poses.clear();
        self.last_poses.extend(
            self.players
                .values()
                .map(|p| (p.uuid, TrackedPose::from(p))),
        );
    }

    pub fn tick(&mut self) {
        let timings = self.timings.clone();
        let _tick = timings.as_ref().map(|t| t.scope("global-tick"));
        let _tracking = timings.as_ref().map(|t| t.scope("entity-tracking"));
        if !self.tracking_changed() {
            return;
        }
        #[cfg(test)]
        {
            self.tracking_passes += 1;
        }
        {
            let input = Arc::get_mut(&mut self.tick_input)
                .expect("global tracking workers hold no lingering input reference");
            input.players.clear();
            input
                .players
                .extend(self.players.values().map(TickPlayer::from));
            for (_, mut bucket) in input.grid.drain() {
                bucket.clear();
                input.grid_pool.push(bucket);
            }
            input.index.clear();
            input.index.reserve(input.players.len());
            for (i, p) in input.players.iter().enumerate() {
                let key = cell_of(p);
                let bucket = input
                    .grid
                    .entry(key)
                    .or_insert_with(|| input.grid_pool.pop().unwrap_or_default());
                bucket.push(i);
                input.index.insert(p.uuid, i);
            }
            input.full_scan = self.last_poses.len() != input.players.len();
            input.observer_dirty.clear();
            input.observer_dirty.resize(input.players.len(), false);
            input.moved_targets.clear();
            for (i, p) in input.players.iter().enumerate() {
                let changed = input.full_scan
                    || self
                        .last_poses
                        .get(&p.uuid)
                        .is_none_or(|last| *last != TrackedPose::from(p));
                input.observer_dirty[i] = changed;
                if changed {
                    input.moved_targets.push(i);
                }
            }
        }
        self.remember_poses();
        self.observed_buf.clear();
        let n_players = self.tick_input.players.len();
        self.observed_buf.reserve(n_players);
        for p in &self.tick_input.players {
            self.observed_buf
                .push(self.visible.remove(&p.uuid).unwrap_or_default());
        }
        let threads = if n_players < 64 {
            1
        } else {
            std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(8)
        };
        if threads == 1 {
            run_tracking_chunk(
                &self.tick_input,
                self.shared_encoder.as_deref(),
                0,
                &mut self.observed_buf,
                &mut self.single_scratch,
            );
            for (i, seen) in self.observed_buf.drain(..).enumerate() {
                if !seen.is_empty() {
                    let uuid = self.tick_input.players[i].uuid;
                    self.visible.insert(uuid, seen);
                }
            }
        } else {
            let pool = self
                .workers
                .get_or_insert_with(|| TrackingWorkers::new(threads));
            let per = n_players.div_ceil(threads);
            let mut offset = 0usize;
            let (lock, work_cv, done_cv) = &*pool.state;
            {
                let mut job = lock.lock().unwrap();
                job.seq = job.seq.wrapping_add(1);
                job.input = Arc::clone(&self.tick_input);
                job.shared_encoder = self.shared_encoder.clone();
                job.pending_chunks.clear();
                job.done_chunks.clear();
                while !self.observed_buf.is_empty() {
                    let take = per.min(self.observed_buf.len());
                    let chunk: Vec<_> = self.observed_buf.drain(..take).collect();
                    job.pending_chunks.push((offset, chunk));
                    offset += take;
                }
                work_cv.notify_all();
                while !job.pending_chunks.is_empty() || job.active_workers > 0 {
                    job = done_cv.wait(job).unwrap();
                }
                job.input = Arc::new(TickInput::default());
                job.shared_encoder = None;
                for (first, chunk) in job.done_chunks.drain(..) {
                    for (local_i, seen) in chunk.into_iter().enumerate() {
                        if !seen.is_empty() {
                            let uuid = self.tick_input.players[first + local_i].uuid;
                            self.visible.insert(uuid, seen);
                        }
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::Packet;

    fn tracked(ar: &mut ferrite_net::game::OutboundReceiver, id: i32) -> bool {
        std::iter::from_fn(|| ar.try_recv()).any(|p| {
            p.raw_bytes()
                .is_some_and(|b| b.first().copied() == Some(id as u8))
        })
    }

    #[test]
    fn entity_tracking_follows_the_48_block_range_across_grid_cells() {
        let mut g = GlobalRegion::default();
        let (a, mut ar) = OutboundSender::channel(1 << 20);
        let (b, _br) = OutboundSender::channel(1 << 20);
        let (alice, bob) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut pa = GlobalPlayer::new(alice, 1, "A".into(), vec![], a, false, 1, 0);
        pa.pos = [47.0, 64.0, 0.5];
        let mut pb = GlobalPlayer::new(bob, 2, "B".into(), vec![], b, false, 1, 0);
        pb.pos = [49.0, 64.0, 0.5];
        g.join(pa);
        g.join(pb);
        while ar.try_recv().is_some() {}
        // 2 blocks apart but in different 48-block cells: still tracked.
        g.tick();
        assert!(tracked(&mut ar, AddEntity::ID));
        // Far beyond the grid neighbourhood: the old pair is found through the visible set.
        g.players.get_mut(&bob).unwrap().pos = [5000.0, 64.0, 5000.0];
        g.tick();
        assert!(tracked(&mut ar, RemoveEntities::ID));
        // Exactly out of range inside neighbouring cells is not tracked.
        g.players.get_mut(&bob).unwrap().pos = [47.0 + 48.5, 64.0, 0.5];
        g.tick();
        assert!(!tracked(&mut ar, AddEntity::ID));
    }

    #[test]
    fn entity_tracking_does_not_cross_dimensions() {
        let mut g = GlobalRegion::default();
        let (a, mut ar) = OutboundSender::channel(1 << 20);
        let (b, _br) = OutboundSender::channel(1 << 20);
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let mut pa = GlobalPlayer::new(alice, 1, "A".into(), vec![], a, false, 1, 0);
        pa.pos = [0.5, 64.0, 0.5];
        let mut pb = GlobalPlayer::new(bob, 2, "B".into(), vec![], b, false, 1, 0);
        pb.pos = [1.5, 64.0, 0.5];
        pb.dimension = Dimension::Nether;
        pb.world_id = WorldId::Vanilla(Dimension::Nether);
        g.join(pa);
        g.join(pb);
        // Drop the join packets so only the tracking sends remain.
        while ar.try_recv().is_some() {}

        g.tick();
        assert!(
            !tracked(&mut ar, AddEntity::ID),
            "a nether player must not be spawned in the overworld"
        );

        g.set_world(
            bob,
            Dimension::Overworld,
            WorldId::Vanilla(Dimension::Overworld),
        );
        g.tick();
        assert!(
            tracked(&mut ar, AddEntity::ID),
            "the entity is re-added when the player enters the observer's dimension"
        );

        g.set_world(bob, Dimension::Nether, WorldId::Vanilla(Dimension::Nether));
        g.tick();
        assert!(
            tracked(&mut ar, RemoveEntities::ID),
            "the entity is removed when the player leaves the observer's dimension"
        );
    }

    #[test]
    fn players_in_distinct_worlds_with_same_dimension_are_not_tracked() {
        let mut global = GlobalRegion::default();
        let (outbound, mut packets) = OutboundSender::channel(1 << 20);
        let (other, _) = OutboundSender::channel(1 << 20);
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        let mut a = GlobalPlayer::new(alice, 1, "A".into(), vec![], outbound, false, 1, 0);
        let mut b = GlobalPlayer::new(bob, 2, "B".into(), vec![], other, false, 1, 0);
        a.pos = [0.5, 64.0, 0.5];
        b.pos = [1.5, 64.0, 0.5];
        b.world_id = WorldId::Runtime(Uuid::from_u128(3));
        global.join(a);
        global.join(b);
        while packets.try_recv().is_some() {}
        global.tick();
        assert!(!tracked(&mut packets, AddEntity::ID));
        global.set_world(
            bob,
            Dimension::Overworld,
            WorldId::Vanilla(Dimension::Overworld),
        );
        global.tick();
        assert!(tracked(&mut packets, AddEntity::ID));
    }

    #[test]
    fn unchanged_ticks_skip_the_tracking_pass() {
        let mut g = GlobalRegion::default();
        let (a, _ar) = OutboundSender::channel(1 << 20);
        let (b, _br) = OutboundSender::channel(1 << 20);
        let mut pa = GlobalPlayer::new(Uuid::from_u128(1), 1, "A".into(), vec![], a, false, 1, 0);
        pa.pos = [0.5, 64.0, 0.5];
        let mut pb = GlobalPlayer::new(Uuid::from_u128(2), 2, "B".into(), vec![], b, false, 1, 0);
        pb.pos = [1.5, 64.0, 0.5];
        g.join(pa);
        g.join(pb);

        g.tick();
        let passes = g.tracking_passes;
        assert_eq!(passes, 1);
        g.tick();
        assert_eq!(
            g.tracking_passes, passes,
            "a tick with no pose/world change must not rescan every pair"
        );

        // Direct edits (not only `GlobalRegion::update`) are part of the input too.
        g.players.get_mut(&Uuid::from_u128(2)).unwrap().pos = [2.5, 64.0, 0.5];
        g.tick();
        assert_eq!(g.tracking_passes, passes + 1);
    }

    #[test]
    fn an_unchanged_pose_does_not_resend_movement() {
        let mut g = GlobalRegion::default();
        let (a, mut ar) = OutboundSender::channel(1 << 20);
        let (b, mut br) = OutboundSender::channel(1 << 20);
        let mut pa = GlobalPlayer::new(Uuid::from_u128(1), 1, "A".into(), vec![], a, false, 1, 0);
        pa.pos = [0.5, 64.0, 0.5];
        let mut pb = GlobalPlayer::new(Uuid::from_u128(2), 2, "B".into(), vec![], b, false, 1, 0);
        pb.pos = [1.5, 64.0, 0.5];
        g.join(pa);
        g.join(pb);
        g.tick();
        while ar.try_recv().is_some() {}
        while br.try_recv().is_some() {}

        g.tick();
        assert!(
            ar.try_recv().is_none() && br.try_recv().is_none(),
            "an unchanged pose must not allocate or send a movement packet"
        );
    }

    #[test]
    fn tick_records_global_and_tracking_scopes_when_timings_are_attached() {
        let mut g = GlobalRegion::default();
        let timings = Arc::new(Timings::new());
        g.timings = Some(timings.clone());

        g.tick();

        let report = timings.report_all(50.0);
        assert!(
            report.rows.iter().any(|row| row.path == "global-tick"),
            "{report}"
        );
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.path == "global-tick/entity-tracking"),
            "{report}"
        );
    }

    #[test]
    fn visibility_and_chat_follow_positions() {
        let mut g = GlobalRegion::default();
        let (a, mut ar) = OutboundSender::channel(1 << 20);
        let (b, mut br) = OutboundSender::channel(1 << 20);
        let pa = GlobalPlayer::new(
            uuid::Uuid::from_u128(1),
            1,
            "A".into(),
            vec![],
            a,
            false,
            1,
            0,
        );
        let pb = GlobalPlayer::new(
            uuid::Uuid::from_u128(2),
            2,
            "B".into(),
            vec![],
            b,
            false,
            1,
            0,
        );
        g.join(pa);
        g.join(pb);
        g.tick();
        assert!(
            std::iter::from_fn(|| br.try_recv())
                .any(|p| matches!(p,Outbound::Packet(ref b) if b[0]==1))
        );
        g.chat(
            uuid::Uuid::from_u128(1),
            &ServerboundChat {
                message: "hello".into(),
                timestamp: 0,
                salt: i64::default(),
                signature: None,
                last_seen: LastSeenUpdate::new(0, &[], 0),
            },
        );
        assert!(
            std::iter::from_fn(|| br.try_recv())
                .any(|p| matches!(p,Outbound::Packet(ref b) if b[0]==66))
        );
        g.leave(uuid::Uuid::from_u128(1));
        assert!(
            std::iter::from_fn(|| br.try_recv())
                .any(|p| matches!(p,Outbound::Packet(ref b) if b[0]==78))
        );
        let _ = &mut ar;
    }
}
