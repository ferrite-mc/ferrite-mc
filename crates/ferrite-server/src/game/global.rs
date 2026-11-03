use super::{Dimension, chat::PlayerChatState};
use ferrite_net::game::{Outbound, OutboundSender, packet};
use ferrite_profiler::Timings;
use ferrite_protocol::{
    VarInt,
    packets::{login::Property, play::*},
    types::PlainText,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
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
    visible: HashMap<(Uuid, Uuid), GlobalPlayer>,
    /// Scope tree attached by the game loop; `None` until the first tick.
    pub timings: Option<Arc<Timings>>,
}
fn send<P: ferrite_protocol::Packet>(to: &GlobalPlayer, p: &P) {
    let _ = to.outbound.send(packet(p));
}
fn angle(v: f32) -> u8 {
    (v * 256.0 / 360.0).floor() as i32 as u8
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
    /// Moves a player between dimensions. The next tick removes its entity from observers in
    /// the old dimension and adds it for observers in the new one.
    pub fn set_dimension(&mut self, id: Uuid, dimension: Dimension) {
        if let Some(player) = self.players.get_mut(&id) {
            player.dimension = dimension;
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
        // One encoding, shared by every recipient.
        let message = packet(&SystemChat {
            content: PlainText(text.into()),
            overlay: false,
        });
        for p in self.players.values() {
            let _ = p.outbound.send(message.clone());
        }
    }
    pub fn leave(&mut self, id: Uuid) {
        if let Some(p) = self.players.remove(&id) {
            for other in self.players.values() {
                send(other, &PlayerInfoRemove { uuids: vec![id] });
                if self.visible.remove(&(other.uuid, id)).is_some() {
                    send(
                        other,
                        &RemoveEntities {
                            ids: vec![VarInt(p.entity)],
                        },
                    );
                }
            }
            self.visible.retain(|(observer, _), _| *observer != id);
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
        for observer in &players {
            for target in &players {
                if observer.uuid == target.uuid {
                    continue;
                }
                let key = (observer.uuid, target.uuid);
                let distance = observer
                    .pos
                    .iter()
                    .zip(target.pos)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f64>();
                // Vanilla `ChunkMap.TrackedEntity` only tracks entities in the same level.
                let inside = observer.dimension == target.dimension && distance <= 48.0 * 48.0;
                match (self.visible.get(&key), inside) {
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
                        self.visible.insert(key, target.clone());
                    }
                    (Some(_), false) => {
                        send(
                            observer,
                            &RemoveEntities {
                                ids: vec![VarInt(target.entity)],
                            },
                        );
                        self.visible.remove(&key);
                    }
                    (Some(old), true) => {
                        let delta: Vec<i64> = target
                            .pos
                            .iter()
                            .zip(old.pos)
                            .map(|(n, o)| (n * 4096.0).floor() as i64 - (o * 4096.0).floor() as i64)
                            .collect();
                        let moved = delta.iter().any(|v| *v != 0);
                        let rotated = old.yaw != target.yaw || old.pitch != target.pitch;
                        if moved {
                            if delta.iter().all(|d| i16::try_from(*d).is_ok()) {
                                let dx = delta[0] as i16;
                                let dy = delta[1] as i16;
                                let dz = delta[2] as i16;
                                let properties = VarInt(i32::from(target.ground));
                                if rotated {
                                    send(
                                        observer,
                                        &EntityMoveRotation {
                                            id: VarInt(target.entity),
                                            properties,
                                            dx,
                                            dy,
                                            dz,
                                            yaw: angle(target.yaw),
                                            pitch: angle(target.pitch),
                                        },
                                    );
                                } else {
                                    send(
                                        observer,
                                        &EntityMove {
                                            id: VarInt(target.entity),
                                            properties,
                                            dx,
                                            dy,
                                            dz,
                                        },
                                    );
                                }
                            } else {
                                send(
                                    observer,
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
                                );
                            }
                        } else if rotated {
                            send(
                                observer,
                                &EntityRotation {
                                    id: VarInt(target.entity),
                                    on_ground: target.ground,
                                    yaw: angle(target.yaw),
                                    pitch: angle(target.pitch),
                                },
                            );
                        }
                        if rotated {
                            send(
                                observer,
                                &EntityHeadRotation {
                                    id: VarInt(target.entity),
                                    yaw: angle(target.yaw),
                                },
                            );
                        }
                        self.visible.insert(key, target.clone());
                    }
                    _ => {}
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
        std::iter::from_fn(|| ar.try_recv())
            .any(|p| matches!(p, Outbound::Packet(ref b) if b[0] == id as u8))
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
        g.join(pa);
        g.join(pb);
        // Drop the join packets so only the tracking sends remain.
        while ar.try_recv().is_some() {}

        g.tick();
        assert!(
            !tracked(&mut ar, AddEntity::ID),
            "a nether player must not be spawned in the overworld"
        );

        g.set_dimension(bob, Dimension::Overworld);
        g.tick();
        assert!(
            tracked(&mut ar, AddEntity::ID),
            "the entity is re-added when the player enters the observer's dimension"
        );

        g.set_dimension(bob, Dimension::Nether);
        g.tick();
        assert!(
            tracked(&mut ar, RemoveEntities::ID),
            "the entity is removed when the player leaves the observer's dimension"
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
                salt: 0,
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
