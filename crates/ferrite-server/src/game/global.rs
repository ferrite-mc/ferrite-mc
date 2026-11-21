use super::{Dimension, WorldId, chat::PlayerChatState};
use ferrite_net::game::{Outbound, OutboundSender, packet, shared_packet};
use ferrite_profiler::Timings;
use ferrite_protocol::{
    FrameEncoder, VarInt,
    packets::{login::Property, play::*},
    types::PlainText,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
/// What an observer was last told about a target: just the pose, so tracking does not copy a
/// whole player (name, skin properties, channel handle) per observed pair per tick.
#[derive(Clone, Copy)]
struct Pose {
    pos: [f64; 3],
    yaw: f32,
    pitch: f32,
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
            game_mode: 1,
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
pub struct GlobalRegion {
    players: HashMap<Uuid, GlobalPlayer>,
    /// Per observer, the targets it was told about and the pose it last saw them in.
    visible: HashMap<Uuid, HashMap<Uuid, Pose>>,
    /// Scope tree attached by the game loop; `None` until the first tick.
    pub timings: Option<Arc<Timings>>,
    /// Shared encoder matching server compression config so multi-viewer movement and chat
    /// broadcasts compress wire frames once per message instead of once per recipient.
    shared_encoder: Option<Arc<FrameEncoder>>,
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
    encoder: Option<&FrameEncoder>,
    target: &GlobalPlayer,
    old: &Pose,
) -> Vec<Outbound> {
    let delta: Vec<i64> = target
        .pos
        .iter()
        .zip(old.pos)
        .map(|(n, o)| (n * 4096.0).floor() as i64 - (o * 4096.0).floor() as i64)
        .collect();
    let moved = delta.iter().any(|v| *v != 0);
    let rotated = old.yaw != target.yaw || old.pitch != target.pitch;
    let mut out = Vec::new();
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
    out
}

impl GlobalRegion {
    pub fn join(&mut self, p: GlobalPlayer) {
        for old in self.players.values() {
            send(old, &p.info());
            send(&p, &old.info());
        }
        send(&p, &p.info());
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
    pub fn tick(&mut self) {
        let timings = self.timings.clone();
        let _tick = timings.as_ref().map(|t| t.scope("global-tick"));
        let _tracking = timings.as_ref().map(|t| t.scope("entity-tracking"));
        let players: Vec<_> = self.players.values().cloned().collect();
        // Tracking range is 48 blocks, so with 48-block cells an observer can only see targets
        // in its own and the eight neighbouring cells (same dimension), plus the targets it
        // already sees, which may have moved away or changed dimension and need removing. This
        // replaces the all-pairs scan, which cost 380 ms per tick at 800 players.
        const CELL: f64 = 48.0;
        let cell = |p: &GlobalPlayer| {
            (
                p.world_id,
                (p.pos[0] / CELL).floor() as i32,
                (p.pos[2] / CELL).floor() as i32,
            )
        };
        let mut grid: HashMap<_, Vec<usize>> = HashMap::new();
        let mut index = HashMap::with_capacity(players.len());
        for (i, p) in players.iter().enumerate() {
            grid.entry(cell(p)).or_default().push(i);
            index.insert(p.uuid, i);
        }
        let mut observed: Vec<HashMap<Uuid, Pose>> = players
            .iter()
            .map(|p| self.visible.remove(&p.uuid).unwrap_or_default())
            .collect();
        // Entries of observers that left are gone with them.
        self.visible.clear();
        let shared_encoder = self.shared_encoder.as_deref();
        // Each observer only reads shared state and writes its own `observed` map, so observers
        // are split across threads; one thread is used for small servers.
        let work = |first: usize, chunk: &mut [HashMap<Uuid, Pose>]| {
            let mut moves: HashMap<(Uuid, [u64; 3], u32, u32), Vec<Outbound>> = HashMap::new();
            let mut candidates: Vec<usize> = Vec::new();
            for (offset, seen) in chunk.iter_mut().enumerate() {
                let observer = &players[first + offset];
                let (world_id, cx, cz) = cell(observer);
                candidates.clear();
                candidates.extend(seen.keys().filter_map(|target| index.get(target).copied()));
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(near) = grid.get(&(world_id, cx + dx, cz + dz)) {
                            candidates.extend_from_slice(near);
                        }
                    }
                }
                candidates.sort_unstable();
                candidates.dedup();
                for &ti in &candidates {
                    let target = &players[ti];
                    if observer.uuid == target.uuid {
                        continue;
                    }
                    let distance = observer
                        .pos
                        .iter()
                        .zip(target.pos)
                        .map(|(a, b)| (a - b) * (a - b))
                        .sum::<f64>();
                    // Vanilla `ChunkMap.TrackedEntity` only tracks entities in the same level.
                    let inside = observer.world_id == target.world_id && distance <= 48.0 * 48.0;
                    match (seen.get(&target.uuid), inside) {
                        (None, true) => {
                            send(
                                observer,
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
                            );
                            seen.insert(target.uuid, Pose::from(target));
                        }
                        (Some(_), false) => {
                            send(
                                observer,
                                &RemoveEntities {
                                    ids: vec![VarInt(target.entity)],
                                },
                            );
                            seen.remove(&target.uuid);
                        }
                        (Some(old), true) => {
                            // Observers that last saw the target in the same pose get identical
                            // packets, so encode them once per tick.
                            let cache_key = (
                                target.uuid,
                                old.pos.map(f64::to_bits),
                                old.yaw.to_bits(),
                                old.pitch.to_bits(),
                            );
                            let packets = moves
                                .entry(cache_key)
                                .or_insert_with(|| movement_packets(shared_encoder, target, old));
                            for packet in packets.iter() {
                                let _ = observer.outbound.send(packet.clone());
                            }
                            seen.insert(target.uuid, Pose::from(target));
                        }
                        _ => {}
                    }
                }
            }
        };
        let threads = if players.len() < 64 {
            1
        } else {
            std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(8)
        };
        if threads == 1 {
            work(0, &mut observed);
        } else {
            let per = players.len().div_ceil(threads);
            std::thread::scope(|scope| {
                for (n, chunk) in observed.chunks_mut(per).enumerate() {
                    let work = &work;
                    scope.spawn(move || work(n * per, chunk));
                }
            });
        }
        for (player, seen) in players.iter().zip(observed) {
            if !seen.is_empty() {
                self.visible.insert(player.uuid, seen);
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
