//! Secure chat: profile-key sessions, message signatures, the per-sender signature chain, and
//! the last-seen acknowledgement tracking described in the signed-chat notes (the old handoff
//! documents were removed; recover them with `git log --all -- docs/handoff`).
//!
//! Wire formats and semantics mirror vanilla 26.3:
//! * `ServerboundChatSessionUpdatePacket` registers a 2048-bit RSA profile key for a session.
//! * Every `serverbound_chat` is signed with `SHA256withRSA` over the sender UUID, session UUID,
//!   the chain index, the body, and the sender's last-seen signatures.
//! * `LastSeenMessagesValidator` (20 slots) tracks which signed messages each client received so
//!   later messages can prove they followed the chain.
//!
//! Vanilla additionally proves that the session's profile key was signed by Mojang's services key
//! (`https://discovery.minecraftservices.com`). Ferrite has no services-key discovery yet, so the
//! key is trusted after parsing and expiry checks; message signatures are still verified.

use ferrite_protocol::packets::play::{
    LastSeenUpdate, MessageSignature, RemoteChatSessionData, ServerboundChat,
};
use rsa::{Pkcs1v15Sign, RsaPublicKey, pkcs8::DecodePublicKey};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use uuid::Uuid;

/// Vanilla `LastSeenMessages.LAST_SEEN_MESSAGES_MAX_LENGTH`.
pub const LAST_SEEN_COUNT: usize = 20;

/// A validated chat session: the client's profile key plus its declared lifetime.
pub struct ChatSession {
    data: RemoteChatSessionData,
    key: RsaPublicKey,
}
impl ChatSession {
    pub fn data(&self) -> &RemoteChatSessionData {
        &self.data
    }
    fn is_expired(&self, now_ms: i64) -> bool {
        self.data.profile_key.expires_at < now_ms
    }
}
impl std::fmt::Debug for ChatSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatSession")
            .field("session_id", &self.data.session_id)
            .finish_non_exhaustive()
    }
}
impl Clone for ChatSession {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            key: self.key.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The replacement key expires before the currently registered one.
    Expired,
    /// The profile key is not a valid X.509 RSA public key.
    InvalidKey,
}
impl SessionError {
    /// Client-visible disconnect reason (`multiplayer.disconnect.*` translations).
    pub fn disconnect_reason(self) -> &'static str {
        match self {
            Self::Expired => "Expired profile public key",
            Self::InvalidKey => "Invalid public key signature",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatError {
    MissingProfileKey,
    ChainBroken,
    ExpiredProfileKey,
    InvalidSignature,
    OutOfOrder,
}
impl ChatError {
    /// Red chat message shown to the sender, matching vanilla's `chat.disabled.*` text.
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingProfileKey => "Chat disabled since your profile key is missing",
            Self::ChainBroken => "Chat disabled since your message chain is broken",
            Self::ExpiredProfileKey => "Chat disabled since your profile key has expired",
            Self::InvalidSignature => "Chat disabled since your message signature is invalid",
            Self::OutOfOrder => "Chat disabled since your messages are out of order",
        }
    }
}

/// A chat message as it will be broadcast (`PlayerChatMessage`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerChatMessage {
    /// The sender's signature-chain index.
    pub index: i32,
    pub signature: Option<MessageSignature>,
    pub content: String,
    /// Epoch milliseconds; zero for unsigned messages, like `SignedMessageBody.unsigned`.
    pub timestamp: i64,
    pub salt: i64,
    /// Signatures the sender acknowledged before this message.
    pub last_seen: Vec<MessageSignature>,
    /// Whether this message carries a verifiable profile-key signature.
    pub signed: bool,
}

/// `SignedMessageChain.Decoder`: the next link in one sender's session.
#[derive(Debug, Clone)]
struct ChatChain {
    sender: Uuid,
    session_id: Uuid,
    next_index: i32,
    last_timestamp: i64,
    broken: bool,
}
impl ChatChain {
    fn new(sender: Uuid, session_id: Uuid) -> Self {
        Self {
            sender,
            session_id,
            next_index: 0,
            // `SignedMessageChain` starts at `Instant.EPOCH`.
            last_timestamp: 0,
            broken: false,
        }
    }
    fn unpack(
        &mut self,
        key: &RsaPublicKey,
        signature: Option<MessageSignature>,
        chat: &ServerboundChat,
        last_seen: Vec<MessageSignature>,
        now_ms: i64,
    ) -> Result<PlayerChatMessage, ChatError> {
        if self.broken {
            return Err(ChatError::ChainBroken);
        }
        // An expired key is reported before the chain is touched (vanilla's decoder does the
        // same); the chain keeps its next index so a later session can restart it.
        let Some(signature) = signature else {
            return Err(ChatError::MissingProfileKey);
        };
        if chat.timestamp < self.last_timestamp {
            self.broken = true;
            return Err(ChatError::OutOfOrder);
        }
        let payload = signature_payload(
            self.sender,
            self.session_id,
            self.next_index,
            &chat.message,
            chat.timestamp,
            chat.salt,
            &last_seen,
        );
        if !verify(key, &payload, &signature) {
            self.broken = true;
            return Err(ChatError::InvalidSignature);
        }
        if chat.timestamp.div_euclid(1000) * 1000 + 300_000 < now_ms {
            // Vanilla only logs expired messages; they still belong to a valid chain.
            tracing::warn!(%self.sender, "received expired chat message");
        }
        let index = self.next_index;
        self.next_index += 1;
        self.last_timestamp = chat.timestamp;
        Ok(PlayerChatMessage {
            index,
            signature: Some(signature),
            content: chat.message.clone(),
            timestamp: chat.timestamp,
            salt: chat.salt,
            last_seen,
            signed: true,
        })
    }
}

/// The bytes signed by `PlayerChatMessage.updateSignature`.
fn signature_payload(
    sender: Uuid,
    session_id: Uuid,
    index: i32,
    content: &str,
    timestamp_ms: i64,
    salt: i64,
    last_seen: &[MessageSignature],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 16 + 16 + 4 + 8 + 8 + 4 + content.len() + 4 + 256 * 20);
    // Ints.toByteArray(1): a version tag plus the link (sender, session, index).
    out.extend_from_slice(&1i32.to_be_bytes());
    out.extend_from_slice(sender.as_bytes());
    out.extend_from_slice(session_id.as_bytes());
    out.extend_from_slice(&index.to_be_bytes());
    // SignedMessageBody: salt, epoch seconds, content bytes, last-seen list.
    out.extend_from_slice(&salt.to_be_bytes());
    out.extend_from_slice(&(timestamp_ms.div_euclid(1000)).to_be_bytes());
    let content = content.as_bytes();
    out.extend_from_slice(&(content.len() as i32).to_be_bytes());
    out.extend_from_slice(content);
    out.extend_from_slice(&(last_seen.len() as i32).to_be_bytes());
    for signature in last_seen {
        out.extend_from_slice(&signature.0);
    }
    out
}

fn verify(key: &RsaPublicKey, payload: &[u8], signature: &MessageSignature) -> bool {
    let digest = Sha256::digest(payload);
    key.verify(Pkcs1v15Sign::new::<Sha256>(), &digest, &signature.0)
        .is_ok()
}

#[derive(Debug, Clone)]
struct Tracked {
    signature: MessageSignature,
    pending: bool,
}

/// Vanilla `LastSeenMessagesValidator`, including the pending/acknowledged slot dance.
#[derive(Debug, Clone)]
pub struct LastSeenValidator {
    last_seen_count: usize,
    tracked: VecDeque<Option<Tracked>>,
    last_pending: Option<MessageSignature>,
}
impl Default for LastSeenValidator {
    fn default() -> Self {
        Self::new(LAST_SEEN_COUNT)
    }
}
impl LastSeenValidator {
    pub fn new(last_seen_count: usize) -> Self {
        Self {
            last_seen_count,
            tracked: std::iter::repeat_n(None, last_seen_count).collect(),
            last_pending: None,
        }
    }
    /// Queues a signature as pending: the client has not acknowledged it yet.
    pub fn add_pending(&mut self, signature: MessageSignature) {
        if self.last_pending == Some(signature) {
            return;
        }
        self.last_pending = Some(signature);
        self.tracked.push_back(Some(Tracked {
            signature,
            pending: true,
        }));
    }
    /// Drops `offset` already-acknowledged entries from the front.
    pub fn apply_offset(&mut self, offset: i32) -> Result<(), String> {
        let valid_range = self.tracked.len() - self.last_seen_count;
        let offset = usize::try_from(offset).map_err(|_| format!("negative offset {offset}"))?;
        if offset > valid_range {
            return Err(format!("offset {offset} exceeds {valid_range}"));
        }
        self.tracked.drain(..offset);
        Ok(())
    }
    /// Applies a client update and returns the acked signatures in slot order.
    pub fn apply_update(
        &mut self,
        update: &LastSeenUpdate,
    ) -> Result<Vec<MessageSignature>, String> {
        self.apply_offset(update.offset.0)?;
        if update.acknowledged[2] & 0xF0 != 0 {
            return Err("acknowledged bitset exceeds 20 entries".into());
        }
        let mut entries = Vec::new();
        for index in 0..self.last_seen_count {
            let acknowledged = update.is_acknowledged(index);
            let entry = self.tracked.get(index).cloned().flatten();
            match (acknowledged, entry) {
                (true, Some(tracked)) => {
                    entries.push(tracked.signature);
                    self.tracked[index] = Some(Tracked {
                        pending: false,
                        ..tracked
                    });
                }
                (true, None) => {
                    return Err(format!("acknowledged empty slot {index}"));
                }
                (false, Some(tracked)) if !tracked.pending => {
                    return Err(format!("unexpectedly unacknowledged slot {index}"));
                }
                (false, _) => {
                    self.tracked[index] = None;
                }
            }
        }
        let expected = last_seen_checksum(&entries);
        if update.checksum != 0 && update.checksum != expected {
            return Err(format!(
                "checksum mismatch: client {}, server {expected}",
                update.checksum
            ));
        }
        Ok(entries)
    }
}

/// `LastSeenMessages.computeChecksum` (JVM `Arrays.hashCode(byte[])` semantics).
pub fn last_seen_checksum(entries: &[MessageSignature]) -> i8 {
    let mut hash = 1i32;
    for entry in entries {
        let mut signature_hash = 1i32;
        for byte in entry.0 {
            signature_hash = signature_hash
                .wrapping_mul(31)
                .wrapping_add(byte as i8 as i32);
        }
        hash = hash.wrapping_mul(31).wrapping_add(signature_hash);
    }
    match hash as u8 {
        0 => 1,
        value => value as i8,
    }
}

/// All chat state owned by one player's connection.
#[derive(Clone)]
pub struct PlayerChatState {
    secure: bool,
    session: Option<ChatSession>,
    chain: Option<ChatChain>,
    tracker: LastSeenValidator,
    next_chat_index: i32,
}
impl PlayerChatState {
    pub fn new(secure: bool) -> Self {
        Self {
            secure,
            session: None,
            chain: None,
            tracker: LastSeenValidator::default(),
            next_chat_index: 0,
        }
    }
    pub fn session_data(&self) -> Option<&RemoteChatSessionData> {
        self.session.as_ref().map(ChatSession::data)
    }
    /// Registers a chat session. Returns whether the session data actually changed.
    ///
    /// Vanilla ignores a re-sent identical key, rejects a key that expires earlier than the
    /// current one, and validates the profile-key signature with Mojang's services key (which
    /// Ferrite does not fetch yet).
    pub fn update_session(
        &mut self,
        sender: Uuid,
        data: RemoteChatSessionData,
    ) -> Result<bool, SessionError> {
        if let Some(session) = &self.session
            && session.data.profile_key == data.profile_key
        {
            return Ok(false);
        }
        if let Some(session) = &self.session
            && data.profile_key.expires_at < session.data.profile_key.expires_at
        {
            return Err(SessionError::Expired);
        }
        let key = RsaPublicKey::from_public_key_der(&data.profile_key.key)
            .map_err(|_| SessionError::InvalidKey)?;
        let session_id = data.session_id;
        self.session = Some(ChatSession { data, key });
        self.chain = Some(ChatChain::new(sender, session_id));
        Ok(true)
    }
    pub fn apply_ack(&mut self, offset: i32) -> Result<(), String> {
        self.tracker.apply_offset(offset)
    }
    pub fn unpack_last_seen(
        &mut self,
        update: &LastSeenUpdate,
    ) -> Result<Vec<MessageSignature>, String> {
        self.tracker.apply_update(update)
    }
    /// Validates and finalizes a chat message, advancing the chain when it is signed.
    pub fn decode(
        &mut self,
        chat: &ServerboundChat,
        last_seen: Vec<MessageSignature>,
        now_ms: i64,
    ) -> Result<PlayerChatMessage, ChatError> {
        let Self { session, chain, .. } = self;
        let Some(session) = session else {
            if self.secure {
                return Err(ChatError::MissingProfileKey);
            }
            // `SignedMessageBody.unsigned`: the current time, no salt and no last-seen entries.
            return Ok(PlayerChatMessage {
                index: 0,
                signature: None,
                content: chat.message.clone(),
                timestamp: now_ms,
                salt: 0,
                last_seen: Vec::new(),
                signed: false,
            });
        };
        if session.is_expired(now_ms) {
            return Err(ChatError::ExpiredProfileKey);
        }
        let Some(chain) = chain.as_mut() else {
            return Err(ChatError::ChainBroken);
        };
        chain.unpack(&session.key, chat.signature, chat, last_seen, now_ms)
    }
    /// Reserves the per-connection chat index used as `global_index` on the wire.
    pub fn next_global_index(&mut self) -> i32 {
        let index = self.next_chat_index;
        self.next_chat_index = self.next_chat_index.wrapping_add(1);
        index
    }
    /// Queues a broadcast signature so the sender's next message can reference it.
    pub fn add_pending(&mut self, signature: MessageSignature) {
        self.tracker.add_pending(signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::{Decode, Encode};
    use rand::rngs::OsRng;
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::EncodePublicKey;
    use std::sync::OnceLock;

    const NOW: i64 = 1_700_000_000_000;

    fn keys() -> &'static (RsaPrivateKey, RsaPublicKey) {
        static KEYS: OnceLock<(RsaPrivateKey, RsaPublicKey)> = OnceLock::new();
        KEYS.get_or_init(|| {
            let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key");
            let public = RsaPublicKey::from(&private);
            (private, public)
        })
    }
    fn session_data(session_id: Uuid) -> RemoteChatSessionData {
        let (_, public) = keys();
        RemoteChatSessionData {
            session_id,
            profile_key: ferrite_protocol::packets::play::ProfileKeyData {
                expires_at: NOW + 3_600_000,
                key: public.to_public_key_der().unwrap().as_bytes().to_vec(),
                key_signature: vec![0; 8],
            },
        }
    }
    fn signed_chat(
        sender: Uuid,
        session_id: Uuid,
        index: i32,
        content: &str,
        timestamp: i64,
        last_seen: &[MessageSignature],
    ) -> ServerboundChat {
        let (private, _) = keys();
        // The salt only has to agree between the signed payload and the packet.
        let salt = rand::random::<i64>();
        let payload = signature_payload(
            sender, session_id, index, content, timestamp, salt, last_seen,
        );
        let digest = Sha256::digest(&payload);
        let signature = private
            .sign(Pkcs1v15Sign::new::<Sha256>(), &digest)
            .unwrap();
        assert_eq!(signature.len(), 256);
        ServerboundChat {
            message: content.into(),
            timestamp,
            salt,
            signature: Some(MessageSignature(signature.as_slice().try_into().unwrap())),
            last_seen: LastSeenUpdate::new(0, &[], 0),
        }
    }

    #[test]
    fn signed_chain_advances_and_rejects_tampering() {
        let sender = Uuid::from_u128(1);
        let session_id = Uuid::from_u128(2);
        let mut state = PlayerChatState::new(true);
        assert!(
            state
                .update_session(sender, session_data(session_id))
                .unwrap()
        );
        let chat = signed_chat(sender, session_id, 0, "hello", NOW, &[]);
        let message = state.decode(&chat, vec![], NOW).unwrap();
        assert_eq!(message.index, 0);
        assert!(message.signed);
        assert_eq!(message.content, "hello");
        let second = signed_chat(sender, session_id, 1, "again", NOW + 1, &[]);
        assert_eq!(state.decode(&second, vec![], NOW).unwrap().index, 1);

        let tampered = ServerboundChat {
            message: "evil".into(),
            ..signed_chat(sender, session_id, 2, "third", NOW + 2, &[])
        };
        assert_eq!(
            state.decode(&tampered, vec![], NOW),
            Err(ChatError::InvalidSignature)
        );
        // A broken chain stays broken, like vanilla's decoder.
        assert_eq!(
            state.decode(
                &signed_chat(sender, session_id, 3, "later", NOW + 3, &[]),
                vec![],
                NOW
            ),
            Err(ChatError::ChainBroken)
        );
    }

    #[test]
    fn chain_rejects_out_of_order_times_and_unsigned_secure_messages() {
        let sender = Uuid::from_u128(3);
        let session_id = Uuid::from_u128(4);
        let mut state = PlayerChatState::new(true);
        state
            .update_session(sender, session_data(session_id))
            .unwrap();
        state
            .decode(
                &signed_chat(sender, session_id, 0, "a", NOW + 10, &[]),
                vec![],
                NOW,
            )
            .unwrap();
        assert_eq!(
            state.decode(
                &signed_chat(sender, session_id, 1, "b", NOW + 9, &[]),
                vec![],
                NOW
            ),
            Err(ChatError::OutOfOrder)
        );

        let mut offline = PlayerChatState::new(true);
        let unsigned = ServerboundChat {
            message: "hi".into(),
            timestamp: NOW,
            salt: i64::default(),
            signature: None,
            last_seen: LastSeenUpdate::new(0, &[], 0),
        };
        assert_eq!(
            offline.decode(&unsigned, vec![], NOW),
            Err(ChatError::MissingProfileKey)
        );
        let mut relaxed = PlayerChatState::new(false);
        let message = relaxed.decode(&unsigned, vec![], NOW).unwrap();
        assert!(!message.signed);
        assert_eq!(message.index, 0);
        assert_eq!(message.timestamp, NOW);
        assert!(message.last_seen.is_empty());
    }

    #[test]
    fn expired_profile_key_is_reported() {
        let sender = Uuid::from_u128(5);
        let session_id = Uuid::from_u128(6);
        let key = "malformed".to_string();
        let mut state = PlayerChatState::new(true);
        let data = session_data(session_id);
        state.update_session(sender, data).unwrap();
        let chat = signed_chat(sender, session_id, 0, "hi", NOW + 3_600_001, &[]);
        assert_eq!(
            state.decode(&chat, vec![], NOW + 3_600_001),
            Err(ChatError::ExpiredProfileKey)
        );
        // A malformed key is rejected during session registration.
        let mut bad = PlayerChatState::new(true);
        let mut data = session_data(session_id);
        data.profile_key.key = key.into_bytes();
        assert_eq!(
            bad.update_session(sender, data),
            Err(SessionError::InvalidKey)
        );
    }

    #[test]
    fn signed_chat_validates_the_acknowledged_last_seen_chain() {
        // Bob's message is broadcast to Alice, so her next signature covers his signature.
        let bob_sender = Uuid::from_u128(10);
        let bob_session = Uuid::from_u128(11);
        let mut bob = PlayerChatState::new(true);
        bob.update_session(bob_sender, session_data(bob_session))
            .unwrap();
        let bob_signature = bob
            .decode(
                &signed_chat(bob_sender, bob_session, 0, "from bob", NOW, &[]),
                vec![],
                NOW,
            )
            .unwrap()
            .signature
            .unwrap();

        let alice_sender = Uuid::from_u128(12);
        let alice_session = Uuid::from_u128(13);
        let mut alice = PlayerChatState::new(true);
        alice
            .update_session(alice_sender, session_data(alice_session))
            .unwrap();
        alice.add_pending(bob_signature);
        let update = LastSeenUpdate::new(1, &[19], last_seen_checksum(&[bob_signature]));
        let acknowledged = alice.unpack_last_seen(&update).unwrap();
        assert_eq!(acknowledged, vec![bob_signature]);

        let mut chat = signed_chat(
            alice_sender,
            alice_session,
            0,
            "thanks bob",
            NOW + 1,
            &acknowledged,
        );
        chat.last_seen = update;
        let message = alice.decode(&chat, acknowledged.clone(), NOW + 1).unwrap();
        assert_eq!(message.last_seen, vec![bob_signature]);

        // Swapping the last-seen list after signing must invalidate the signature, even when the
        // client claims it acknowledged that entry.
        let mut tampered = PlayerChatState::new(true);
        tampered
            .update_session(alice_sender, session_data(alice_session))
            .unwrap();
        assert_eq!(
            tampered.decode(
                &signed_chat(alice_sender, alice_session, 0, "hello", NOW + 1, &[]),
                acknowledged,
                NOW + 1
            ),
            Err(ChatError::InvalidSignature)
        );
    }

    #[test]
    fn last_seen_checksum_matches_vanilla_algorithm() {
        // Inner: Arrays.hashCode(byte[256]) -> 1 then h = 31*h + signed(b). Outer:
        // 1 then h = 31*h + inner; the result is a byte, with zero mapped to one.
        let zero = MessageSignature([0; 256]);
        let mut inner = 1i32;
        for _ in 0..256 {
            inner = inner.wrapping_mul(31);
        }
        let mut outer = 1i32;
        outer = outer.wrapping_mul(31).wrapping_add(inner);
        let expected = match outer as u8 {
            0 => 1,
            value => value as i8,
        };
        assert_eq!(last_seen_checksum(&[zero]), expected);
        assert_eq!(last_seen_checksum(&[]), 1);
    }

    #[test]
    fn tracker_tracks_pending_then_acknowledged_entries() {
        let mut tracker = LastSeenValidator::default();
        assert_eq!(tracker.tracked.len(), LAST_SEEN_COUNT);
        let first = MessageSignature([1; 256]);
        let second = MessageSignature([2; 256]);
        tracker.add_pending(first);
        tracker.add_pending(first); // duplicate of the last pending entry: ignored
        tracker.add_pending(second);
        assert_eq!(tracker.tracked.len(), LAST_SEEN_COUNT + 2);

        // The client sends `offset` = messages added since its previous update (two), and its
        // ring order puts them in the last two of the twenty checked slots.
        let mut update = LastSeenUpdate::new(2, &[18, 19], 0);
        update.checksum = last_seen_checksum(&[first, second]);
        assert_eq!(tracker.apply_update(&update).unwrap(), vec![first, second]);

        // Dropping an acknowledged (non-pending) slot must fail validation.
        assert!(
            tracker
                .apply_update(&LastSeenUpdate::new(0, &[], 0))
                .is_err()
        );
        // Offsets beyond the acknowledged history are rejected.
        assert!(tracker.apply_offset(100).is_err());
    }

    #[test]
    fn tracker_rejects_acknowledging_empty_slots_and_bad_checksums() {
        let mut tracker = LastSeenValidator::default();
        assert!(
            tracker
                .apply_update(&LastSeenUpdate::new(0, &[0], 0))
                .is_err()
        );

        let mut tracker = LastSeenValidator::default();
        tracker.add_pending(MessageSignature([3; 256]));
        assert!(
            tracker
                .apply_update(&LastSeenUpdate::new(1, &[19], 0x7f))
                .is_err()
        );
        let mut tracker = LastSeenValidator::default();
        tracker.add_pending(MessageSignature([3; 256]));
        let mut update = LastSeenUpdate::new(1, &[19], 0);
        update.checksum = last_seen_checksum(&[MessageSignature([3; 256])]);
        assert_eq!(tracker.apply_update(&update).unwrap().len(), 1);

        // All 20 acknowledged slots round-trip together (offset 20, ring slots 0..19).
        let mut tracker = LastSeenValidator::default();
        let signatures: Vec<_> = (0..LAST_SEEN_COUNT)
            .map(|i| MessageSignature([i as u8; 256]))
            .collect();
        for signature in &signatures {
            tracker.add_pending(*signature);
        }
        let mut update = LastSeenUpdate::new(20, &(0..LAST_SEEN_COUNT).collect::<Vec<_>>(), 0);
        update.checksum = last_seen_checksum(&signatures);
        assert_eq!(tracker.apply_update(&update).unwrap(), signatures);
    }

    #[test]
    fn packet_roundtrip_covers_chat_session_update() {
        let data = session_data(Uuid::from_u128(8));
        let packet = ferrite_protocol::packets::play::ServerboundChatSessionUpdate {
            chat_session: data.clone(),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut slice = bytes.as_slice();
        assert_eq!(
            ferrite_protocol::packets::play::ServerboundChatSessionUpdate::decode(&mut slice)
                .unwrap(),
            packet
        );
        assert!(slice.is_empty());
    }
}
