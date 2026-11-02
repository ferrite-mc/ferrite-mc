use ferrite_protocol::{
    Decode, DecodeError, Packet, VarInt, encode_packet,
    packets::{login::Property, play::*},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;
/// Messages decoded from a play connection, with no world state in the transport.
#[derive(Debug, Clone)]
pub enum ServerboundPlay {
    PlayerAction(PlayerAction),
    UseItemOn(UseItemOn),
    PlayerCommand(ServerboundPlayerCommand),
    SignUpdate(SignUpdate),
    SetHeldItem(SetHeldItem),
    CreativeSlot(CreativeSlot<ferrite_item::ItemStack>),
    ContainerClick(ContainerClick<ferrite_item::HashedSlot>),
    ContainerClose(i32),
    /// Vanilla `ServerboundContainerButtonClickPacket` (enchant choice, stonecutter recipe).
    ContainerButtonClick(ContainerButtonClick),
    /// Vanilla `ServerboundRenameItemPacket` (anvil).
    RenameItem(RenameItem),
    /// Vanilla `ServerboundSetBeaconPacket`: the two effects chosen in a beacon screen.
    SetBeacon(SetBeacon),
    Position(MovePosition),
    PositionRotation(MovePositionRotation),
    Rotation(MoveRotation),
    Flags(MoveFlags),
    Confirm(ConfirmTeleport),
    Loaded,
    ChunkBatchReceived(f32),
    /// Boxed: `ServerboundChat` carries the 256-byte signature, which would otherwise dominate
    /// every `GameMessage::Input`.
    Chat(Box<ServerboundChat>),
    ChatAck(ServerboundChatAck),
    ChatCommand(ServerboundChatCommand),
    ChatCommandSigned(ServerboundChatCommandSigned),
    ChatSessionUpdate(ServerboundChatSessionUpdate),
    /// Vanilla `ServerboundClientCommandPacket`: 0 respawns, 1 requests stats.
    ClientCommand(ferrite_protocol::packets::play::ServerboundClientCommand),
    /// Vanilla `ServerboundUseItemPacket`: right-click in the air (eating, throwing, ...).
    UseItem(Box<ferrite_protocol::packets::play::ServerboundUseItem>),
    /// Vanilla `ServerboundAttackPacket`: left-click on an entity.
    Attack(ferrite_protocol::packets::play::ServerboundAttack),
    /// Vanilla `ServerboundInteractPacket`: use or attack an entity at a point in its box.
    Interact(ferrite_protocol::packets::play::ServerboundInteract),
    /// Vanilla `ServerboundSetCommandBlockPacket`: a command block editor's changes.
    SetCommandBlock(ferrite_protocol::packets::play::SetCommandBlock),
}
#[derive(Debug)]
pub enum GameMessage {
    Joined {
        session: u64,
        uuid: Uuid,
        name: String,
        properties: Vec<Property>,
        /// The forwarded address behind a proxy, otherwise the peer's address.
        address: Option<std::net::IpAddr>,
        dimension_type: i32,
        biome: i32,
        view_distance: u8,
        online_mode: bool,
        /// Whether the client must prove chat messages with profile-key signatures
        /// (`enforce-secure-profile` on an authenticated server).
        secure_chat: bool,
        /// The registry id of `minecraft:chat` in the synchronized `chat_type` registry.
        chat_type: i32,
        /// Client chat visibility: 0 = shown, 1 = commands only, 2 = hidden.
        chat_visibility: u8,
        max_players: u32,
        outbound: OutboundSender,
    },
    Input {
        session: u64,
        uuid: Uuid,
        packet: ServerboundPlay,
    },
    Left {
        session: u64,
        uuid: Uuid,
    },
    Console {
        command: String,
        complete: tokio::sync::oneshot::Sender<Result<bool, String>>,
    },
    Shutdown {
        complete: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}
/// A packet is already encoded (or `Arc`-shared when one encoding goes to many players);
/// compression and encryption happen later, in the connection's writer task.
#[derive(Debug, Clone)]
pub enum Outbound {
    Packet(Arc<[u8]>),
    Disconnect(String),
}
pub fn packet<P: Packet>(p: &P) -> Outbound {
    let mut bytes = Vec::new();
    encode_packet(p, &mut bytes);
    Outbound::Packet(bytes.into())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The connection's byte budget is exhausted; the client is being disconnected.
    Full,
    /// The writer task has already stopped.
    Closed,
}
/// Bytes-based budget shared by the game side (producer) and the writer task (consumer).
#[derive(Debug)]
struct Budget {
    limit: usize,
    bytes: AtomicUsize,
    overflow: AtomicBool,
    overflow_tx: watch::Sender<bool>,
}
impl Budget {
    fn mark_overflow(&self) {
        if !self.overflow.swap(true, Ordering::AcqRel) {
            let _ = self.overflow_tx.send(true);
        }
    }
    fn release(&self, size: usize) {
        self.bytes.fetch_sub(size, Ordering::AcqRel);
    }
    fn overflowed(&self) -> bool {
        self.overflow.load(Ordering::Acquire)
    }
}
/// Per-connection outbound queue. `send` never blocks: ticks push bytes and move on, and a
/// client that stops reading gets its queue capped instead of growing without bound.
#[derive(Debug, Clone)]
pub struct OutboundSender {
    tx: mpsc::UnboundedSender<Outbound>,
    budget: Arc<Budget>,
}
/// Receiving end owned by the connection's writer task.
#[derive(Debug)]
pub struct OutboundReceiver {
    rx: mpsc::UnboundedReceiver<Outbound>,
    budget: Arc<Budget>,
    overflow: watch::Receiver<bool>,
}
impl OutboundSender {
    /// `limit` is the maximum number of still-uncompressed packet bytes queued for this
    /// connection.
    pub fn channel(limit: usize) -> (Self, OutboundReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (overflow_tx, overflow) = watch::channel(false);
        let budget = Arc::new(Budget {
            limit,
            bytes: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
            overflow_tx,
        });
        (
            Self {
                tx,
                budget: budget.clone(),
            },
            OutboundReceiver {
                rx,
                budget,
                overflow,
            },
        )
    }
    /// Queue a packet without blocking. A full queue latches the connection into the
    /// "Timed out" path and drops this packet.
    pub fn send(&self, msg: Outbound) -> Result<(), SendError> {
        if self.budget.overflowed() {
            return Err(SendError::Closed);
        }
        let size = match &msg {
            Outbound::Packet(body) => body.len(),
            Outbound::Disconnect(_) => 0,
        };
        let previous = self.budget.bytes.fetch_add(size, Ordering::AcqRel);
        if previous + size > self.budget.limit {
            self.budget.bytes.fetch_sub(size, Ordering::AcqRel);
            self.budget.mark_overflow();
            return Err(SendError::Full);
        }
        if self.tx.send(msg).is_err() {
            self.budget.bytes.fetch_sub(size, Ordering::AcqRel);
            return Err(SendError::Closed);
        }
        Ok(())
    }
    pub fn is_overflowed(&self) -> bool {
        self.budget.overflowed()
    }
}
/// One step of the writer task: a queued packet, a latched overflow, or a closed queue.
#[derive(Debug)]
pub enum Incoming {
    Message(Outbound),
    Overflow,
    Closed,
}
impl OutboundReceiver {
    pub async fn recv(&mut self) -> Option<Outbound> {
        let msg = self.rx.recv().await?;
        self.release(&msg);
        Some(msg)
    }
    pub fn try_recv(&mut self) -> Option<Outbound> {
        let msg = self.rx.try_recv().ok()?;
        self.release(&msg);
        Some(msg)
    }
    fn release(&self, msg: &Outbound) {
        if let Outbound::Packet(body) = msg {
            self.budget.release(body.len());
        }
    }
    /// Wait for the next packet. Cancel-safe: used directly inside the writer's `select!`.
    /// A latched overflow wakes `next` even when packets are still queued, so the writer can
    /// drop the backlog and deliver the disconnect promptly.
    pub async fn next(&mut self) -> Incoming {
        if self.budget.overflowed() {
            return Incoming::Overflow;
        }
        tokio::select! {
            msg = self.rx.recv() => match msg {
                Some(msg) => {
                    self.release(&msg);
                    Incoming::Message(msg)
                }
                // All senders are gone and the queue is empty.
                None => Incoming::Closed,
            },
            _ = self.overflow.changed() => Incoming::Overflow,
        }
    }
    pub fn overflowed(&self) -> bool {
        self.budget.overflowed()
    }
    /// Drop every queued packet and refund its budget. Used once the connection is closing.
    pub fn discard(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.release(&msg);
        }
    }
}
pub fn decode_play(data: &[u8]) -> Result<Option<ServerboundPlay>, DecodeError> {
    let mut b = data;
    let id = VarInt::decode(&mut b)?.0;
    Ok(match id {
        0 => Some(ServerboundPlay::Confirm(ConfirmTeleport::decode(&mut b)?)),
        6 => Some(ServerboundPlay::ChatAck(ServerboundChatAck::decode(
            &mut b,
        )?)),
        7 => Some(ServerboundPlay::ChatCommand(
            ServerboundChatCommand::decode(&mut b)?,
        )),
        8 => Some(ServerboundPlay::ChatCommandSigned(
            ServerboundChatCommandSigned::decode(&mut b)?,
        )),
        9 => Some(ServerboundPlay::Chat(Box::new(ServerboundChat::decode(
            &mut b,
        )?))),
        10 => Some(ServerboundPlay::ChatSessionUpdate(
            ServerboundChatSessionUpdate::decode(&mut b)?,
        )),
        11 => Some(ServerboundPlay::ChunkBatchReceived(
            ChunkBatchReceived::decode(&mut b)?.chunks_per_tick,
        )),
        30 => Some(ServerboundPlay::Position(MovePosition::decode(&mut b)?)),
        31 => Some(ServerboundPlay::PositionRotation(
            MovePositionRotation::decode(&mut b)?,
        )),
        32 => Some(ServerboundPlay::Rotation(MoveRotation::decode(&mut b)?)),
        33 => Some(ServerboundPlay::Flags(MoveFlags::decode(&mut b)?)),
        18 => Some(ServerboundPlay::ContainerClick(ContainerClick::decode(
            &mut b,
        )?)),
        17 => Some(ServerboundPlay::ContainerButtonClick(
            ContainerButtonClick::decode(&mut b)?,
        )),
        19 => Some(ServerboundPlay::ContainerClose(
            ServerboundContainerClose::decode(&mut b)?.window.0,
        )),
        1 => Some(ServerboundPlay::Attack(
            ferrite_protocol::packets::play::ServerboundAttack::decode(&mut b)?,
        )),
        26 => Some(ServerboundPlay::Interact(
            ferrite_protocol::packets::play::ServerboundInteract::decode(&mut b)?,
        )),
        49 => Some(ServerboundPlay::RenameItem(RenameItem::decode(&mut b)?)),
        53 => Some(ServerboundPlay::SetBeacon(SetBeacon::decode(&mut b)?)),
        41 => Some(ServerboundPlay::PlayerAction(PlayerAction::decode(&mut b)?)),
        42 => Some(ServerboundPlay::PlayerCommand(
            ServerboundPlayerCommand::decode(&mut b)?,
        )),
        62 => Some(ServerboundPlay::SignUpdate(SignUpdate::decode(&mut b)?)),
        54 => Some(ServerboundPlay::SetHeldItem(SetHeldItem::decode(&mut b)?)),
        55 => Some(ServerboundPlay::SetCommandBlock(
            ferrite_protocol::packets::play::SetCommandBlock::decode(&mut b)?,
        )),
        57 => Some(ServerboundPlay::CreativeSlot(CreativeSlot::decode(&mut b)?)),
        66 => Some(ServerboundPlay::UseItemOn(UseItemOn::decode(&mut b)?)),
        67 => Some(ServerboundPlay::UseItem(Box::new(
            ferrite_protocol::packets::play::ServerboundUseItem::decode(&mut b)?,
        ))),
        12 => Some(ServerboundPlay::ClientCommand(
            ferrite_protocol::packets::play::ServerboundClientCommand::decode(&mut b)?,
        )),
        44 => {
            PlayerLoaded::decode(&mut b)?;
            Some(ServerboundPlay::Loaded)
        }
        _ => None,
    })
}

#[cfg(test)]
mod queue_tests {
    use super::*;
    #[tokio::test]
    async fn queue_rejects_when_byte_budget_is_exhausted() {
        let (tx, mut rx) = OutboundSender::channel(8);
        let packet = || Outbound::Packet(Arc::from([0u8; 4].as_slice()));
        tx.send(packet()).unwrap();
        tx.send(packet()).unwrap();
        assert_eq!(tx.send(packet()).unwrap_err(), SendError::Full);
        assert!(rx.overflowed());
        let incoming = tokio::time::timeout(std::time::Duration::from_secs(1), rx.next())
            .await
            .expect("overflow must wake the writer");
        assert!(matches!(incoming, Incoming::Overflow));
        // Overflow is latched: the connection is being torn down, so later packets are dropped.
        assert_eq!(tx.send(packet()).unwrap_err(), SendError::Closed);
        let first = rx.recv().await.unwrap();
        assert!(matches!(first, Outbound::Packet(_)));
        rx.discard();
    }
}
#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn routes_interaction_packets() {
        let mut bytes = Vec::new();
        encode_packet(
            &PlayerAction {
                status: VarInt(0),
                location: 0,
                face: 1,
                sequence: VarInt(3),
            },
            &mut bytes,
        );
        assert!(matches!(
            decode_play(&bytes).unwrap(),
            Some(ServerboundPlay::PlayerAction(_))
        ));
    }
    #[test]
    fn routes_survival_input_packets() {
        use ferrite_protocol::packets::play::{
            ServerboundClientCommand, ServerboundPlayerCommand, ServerboundUseItem,
        };
        let mut bytes = Vec::new();
        encode_packet(&ServerboundClientCommand { action: VarInt(0) }, &mut bytes);
        assert!(matches!(
            decode_play(&bytes).unwrap(),
            Some(ServerboundPlay::ClientCommand(ServerboundClientCommand {
                action: VarInt(0)
            }))
        ));
        let mut bytes = Vec::new();
        encode_packet(
            &ServerboundUseItem {
                hand: VarInt(0),
                sequence: VarInt(7),
                yaw: 0.0,
                pitch: 0.0,
            },
            &mut bytes,
        );
        assert!(matches!(
            decode_play(&bytes).unwrap(),
            Some(ServerboundPlay::UseItem(_))
        ));
        let mut bytes = Vec::new();
        encode_packet(
            &ServerboundPlayerCommand {
                entity_id: VarInt(3),
                action: VarInt(3),
                data: VarInt(0),
            },
            &mut bytes,
        );
        assert!(matches!(
            decode_play(&bytes).unwrap(),
            Some(ServerboundPlay::PlayerCommand(_))
        ));
    }

    #[test]
    fn routes_signed_chat_and_acknowledgements() {
        use ferrite_protocol::packets::play::{LastSeenUpdate, MessageSignature};
        let mut bytes = Vec::new();
        encode_packet(
            &ServerboundChat {
                message: "hello".into(),
                timestamp: 1,
                salt: 2,
                signature: Some(MessageSignature([7; 256])),
                last_seen: LastSeenUpdate::new(0, &[1], 0),
            },
            &mut bytes,
        );
        let Some(ServerboundPlay::Chat(chat)) = decode_play(&bytes).unwrap() else {
            panic!("expected chat");
        };
        assert_eq!(chat.message, "hello");
        assert!(chat.signature.is_some());

        let mut bytes = Vec::new();
        encode_packet(&ServerboundChatAck { offset: VarInt(4) }, &mut bytes);
        assert!(matches!(
            decode_play(&bytes).unwrap(),
            Some(ServerboundPlay::ChatAck(ServerboundChatAck {
                offset: VarInt(4)
            }))
        ));
    }
}
