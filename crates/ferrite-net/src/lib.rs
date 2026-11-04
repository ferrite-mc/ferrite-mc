pub mod cfb8;
pub mod forwarding;
pub mod settings;
mod wire;
use anyhow::{Context, Result, bail, ensure};
use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, State, VarInt, encode_packet,
    packets::{
        handshake::{Intent, Intention},
        status::{PingRequest, PongResponse, StatusRequest, StatusResponse},
    },
};
use ferrite_translate::{HandshakeOutcome, RawPacket, Translator};
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use wire::Wire;

/// Vanilla caps the server-list player sample at 12 entries (`MinecraftServer.getPlayerSample`).
pub const STATUS_SAMPLE_LIMIT: usize = 12;
/// Vanilla's Netty `ReadTimeoutHandler` drops a client that sends nothing for 30 seconds.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// One entry of the server-list player sample (`ServerStatus.Players.SamplePlayer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplePlayer {
    pub name: String,
    pub id: uuid::Uuid,
}

/// The live player list behind the server-list ping.
///
/// Connections join when they enter play and leave when they disconnect, so the ping reports the
/// actual online count instead of a hardcoded zero. Entries are keyed by connection session: a
/// duplicate login replaces the old entry for that UUID, and the displaced connection's later
/// leave cannot remove the new one.
#[derive(Debug, Default)]
pub struct StatusState {
    players: std::sync::Mutex<Vec<(u64, SamplePlayer)>>,
}
impl StatusState {
    pub fn join(&self, session: u64, name: impl Into<String>, id: uuid::Uuid) {
        let mut players = self
            .players
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        players.retain(|(_, player)| player.id != id);
        players.push((
            session,
            SamplePlayer {
                name: name.into(),
                id,
            },
        ));
    }
    pub fn leave(&self, session: u64) {
        let mut players = self
            .players
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        players.retain(|(tracked, _)| *tracked != session);
    }
    pub fn online(&self) -> usize {
        self.players
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
    pub fn players(&self) -> Vec<SamplePlayer> {
        self.players
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(|(_, player)| player.clone())
            .collect()
    }
}

/// The vanilla `status_response` JSON.
fn status_json(config: &ServerConfig) -> String {
    let players = config.status.players();
    let sample: Vec<serde_json::Value> = players
        .iter()
        .take(STATUS_SAMPLE_LIMIT)
        .map(|player| serde_json::json!({ "name": player.name, "id": player.id.to_string() }))
        .collect();
    let mut response = serde_json::json!({
        "version": {
            "name": ferrite_protocol::GAME_VERSION,
            "protocol": ferrite_protocol::PROTOCOL_VERSION
        },
        "players": {
            "max": config.max_players,
            "online": players.len(),
            "sample": sample
        },
        "description": { "text": config.motd },
        // Mirrors the `secure_chat` flag advertised at login: offline mode accepts unsigned chat,
        // so it must not claim to enforce secure profiles.
        "enforcesSecureChat": config.enforce_secure_profile && config.online_mode
    });
    if let Some(favicon) = &config.favicon {
        response["favicon"] = serde_json::Value::String(favicon.clone());
    }
    response.to_string()
}

/// Which client protocol versions the listener accepts (`supported-versions` in
/// `ferrite.toml`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SupportedVersions {
    /// The core protocol plus every older version the translation chain reaches.
    #[default]
    All,
    /// Only the core protocol; other clients get vanilla's "outdated" disconnect.
    Native,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub motd: String,
    pub max_players: u32,
    pub online_mode: bool,
    pub encrypt_offline: bool,
    /// Require signed chat from players whose profile key can be validated. Vanilla only
    /// enforces this on authenticated servers and when Mojang's services keys are available.
    pub enforce_secure_profile: bool,
    pub compression_threshold: usize,
    pub view_distance: u8,
    /// Which client protocol versions are accepted.
    pub supported_versions: SupportedVersions,
    /// Maximum queued, still-uncompressed packet bytes per connection. Exceeding it
    /// disconnects the client as timed out instead of blocking or growing without bound.
    pub send_queue_bytes: usize,
    /// Optional kernel send-buffer size for accepted connections. `None` keeps the operating
    /// system's default and autotuning; a small value makes a client that stops reading block
    /// the writer after a known number of bytes.
    pub socket_send_buffer_bytes: Option<usize>,
    /// How long one writer batch may block before the connection is dropped.
    pub write_timeout: std::time::Duration,
    pub forwarding: forwarding::Forwarding,
    /// `data:image/png;base64,...` shown in the server list, when a valid icon was loaded.
    pub favicon: Option<String>,
    /// Live players reported by the server-list ping.
    pub status: Arc<StatusState>,
    /// Handshake/status/login clients that send nothing for this long are dropped, like vanilla's
    /// Netty `ReadTimeoutHandler`.
    pub read_timeout: Duration,
    pub game: Option<tokio::sync::mpsc::UnboundedSender<game::GameMessage>>,
}
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: "A Ferrite server".into(),
            max_players: 20,
            online_mode: true,
            encrypt_offline: false,
            enforce_secure_profile: true,
            compression_threshold: 256,
            view_distance: 10,
            supported_versions: SupportedVersions::All,
            send_queue_bytes: 2 * 1024 * 1024,
            socket_send_buffer_bytes: None,
            write_timeout: std::time::Duration::from_secs(30),
            forwarding: forwarding::Forwarding::None,
            favicon: None,
            status: Arc::new(StatusState::default()),
            read_timeout: DEFAULT_READ_TIMEOUT,
            game: None,
        }
    }
}

pub async fn serve(listener: TcpListener, config: ServerConfig) -> Result<()> {
    i32::try_from(config.compression_threshold)
        .context("compression threshold exceeds protocol limit")?;
    i32::try_from(config.max_players).context("max players exceeds protocol limit")?;
    // A proxy already authenticated the player; never run Mojang session auth on
    // a forwarded connection.
    let key = if !config.forwarding.is_proxy() && (config.online_mode || config.encrypt_offline) {
        Some(std::sync::Arc::new(
            tokio::task::spawn_blocking(|| rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024))
                .await??,
        ))
    } else {
        None
    };
    loop {
        let (stream, peer) = listener.accept().await?;
        stream.set_nodelay(true)?;
        if let Some(bytes) = config.socket_send_buffer_bytes {
            socket2::SockRef::from(&stream)
                .set_send_buffer_size(bytes)
                .context("setting the accepted socket's send buffer")?;
        }
        let key = key.clone();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(Wire::new(stream), config, key, peer.ip()).await {
                tracing::info!(%peer, %error, "connection closed");
            }
        });
    }
}

async fn read_packet<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    decoder: &mut FrameDecoder,
) -> Result<Vec<u8>> {
    loop {
        if let Some(frame) = decoder.next_frame()? {
            return Ok(frame);
        }
        let mut bytes = [0u8; 4096];
        let n = stream.read(&mut bytes).await?;
        if n == 0 {
            bail!("connection closed");
        }
        decoder.feed(&bytes[..n]);
    }
}

/// Read one framed packet, disconnecting a client that stalls past `timeout`.
async fn read_packet_timeout<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    decoder: &mut FrameDecoder,
    timeout: Duration,
) -> Result<Vec<u8>> {
    tokio::time::timeout(timeout, read_packet(stream, decoder))
        .await
        .map_err(|_| anyhow::anyhow!("timed out after {timeout:?} waiting for a packet"))?
}

async fn send_packet<P: Packet, W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    packet: &P,
) -> Result<()> {
    let mut body = Vec::new();
    encode_packet(packet, &mut body);
    let mut wire = Vec::new();
    FrameEncoder::default().encode(&body, &mut wire);
    stream.write_all(&wire).await?;
    Ok(())
}

/// Encode and send a packet, translating it for the client when it is older than the core.
async fn send_translated<W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    encoder: &FrameEncoder,
    translator: Option<&mut Translator>,
    state: State,
    packet: RawPacket,
) -> Result<()> {
    let packets = match translator {
        Some(translator) => translator.clientbound(state, packet)?,
        None => vec![packet],
    };
    for packet in packets {
        let mut wire = Vec::new();
        encoder.encode(&packet.to_frame(), &mut wire);
        stream.write_all(&wire).await?;
    }
    Ok(())
}

async fn handle_connection(
    mut stream: Wire,
    config: ServerConfig,
    key: Option<std::sync::Arc<rsa::RsaPrivateKey>>,
    peer: std::net::IpAddr,
) -> Result<()> {
    let mut decoder = FrameDecoder::default();
    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let handshake_packet = RawPacket::parse(&packet)?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != Intention::ID {
        bail!("expected handshake");
    }
    let handshake = Intention::decode(&mut body)?;
    // A native client keeps the untranslated path; an older one gets a session that
    // rewrites IDs and layouts in both directions.
    let mut translator = match config.supported_versions {
        SupportedVersions::Native => {
            if handshake.intent != Intent::Status
                && handshake.protocol_version != ferrite_protocol::PROTOCOL_VERSION
            {
                let unsupported = ferrite_translate::Unsupported {
                    client_protocol: handshake.protocol_version,
                };
                let disconnect_encoder = FrameEncoder::default();
                send_translated(
                    &mut stream,
                    &disconnect_encoder,
                    None,
                    State::Login,
                    unsupported.login_disconnect(),
                )
                .await?;
                return Ok(());
            }
            None
        }
        SupportedVersions::All => match Translator::from_handshake(&handshake_packet)? {
            HandshakeOutcome::Supported { translator, .. } => {
                (!translator.is_passthrough()).then_some(translator)
            }
            // A login from an unsupported client is rejected in the login state, exactly
            // like vanilla's handshake listener. Status stays untranslated so the server
            // list still shows the version mismatch.
            HandshakeOutcome::Unsupported { unsupported, .. }
                if handshake.intent != Intent::Status =>
            {
                let disconnect_encoder = FrameEncoder::default();
                send_translated(
                    &mut stream,
                    &disconnect_encoder,
                    None,
                    State::Login,
                    unsupported.login_disconnect(),
                )
                .await?;
                return Ok(());
            }
            HandshakeOutcome::Unsupported { .. } => None,
        },
    };
    if handshake.intent != Intent::Status {
        // BungeeCord legacy forwarding is appended to the handshake host. Paper's
        // Spigot patch in ServerHandshakePacketListenerImpl parses it before the
        // login listener is created; do the same here.
        let legacy = if matches!(&config.forwarding, forwarding::Forwarding::Bungee) {
            Some(
                forwarding::parse_legacy_host(&handshake.server_address)?.context(
                    "BungeeCord forwarding is enabled but the handshake has no forwarding data",
                )?,
            )
        } else {
            None
        };
        return offline_login(stream, decoder, config, key, legacy, peer, translator).await;
    }
    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != StatusRequest::ID {
        bail!("expected status request");
    }
    StatusRequest::decode(&mut body)?;
    let json = status_json(&config);
    let status_encoder = FrameEncoder::default();
    send_translated(
        &mut stream,
        &status_encoder,
        translator.as_mut(),
        State::Status,
        RawPacket::of(&StatusResponse { json }),
    )
    .await
    .context("send status")?;
    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != PingRequest::ID {
        bail!("expected ping");
    }
    let ping = PingRequest::decode(&mut body)?;
    send_packet(
        &mut stream,
        &PongResponse {
            payload: ping.payload,
        },
    )
    .await
}

async fn offline_login(
    mut stream: Wire,
    mut decoder: FrameDecoder,
    config: ServerConfig,
    key: Option<std::sync::Arc<rsa::RsaPrivateKey>>,
    legacy: Option<forwarding::LegacyForwarding>,
    peer: std::net::IpAddr,
    mut translator: Option<Translator>,
) -> Result<()> {
    use ferrite_protocol::packets::login::{LoginStart, LoginSuccess, SetCompression};

    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != LoginStart::ID {
        bail!("expected login start");
    }
    let start = LoginStart::decode(&mut body)?;
    if !valid_username(&start.name) {
        bail!("invalid player name");
    }
    let profile = match &config.forwarding {
        forwarding::Forwarding::None => {
            // Java's nameUUIDFromBytes hashes the input directly, without a namespace.
            let (uuid, name, properties) = if let Some(key) = key {
                authenticate(
                    &mut stream,
                    &mut decoder,
                    &key,
                    &start.name,
                    config.online_mode,
                    config.read_timeout,
                )
                .await?
            } else {
                (offline_uuid(&start.name), start.name.clone(), vec![])
            };
            Profile {
                uuid,
                name,
                properties,
                // No proxy: the peer address is the player's address.
                address: Some(peer),
                chat_visibility: 0,
            }
        }
        forwarding::Forwarding::Velocity { secret } => {
            let forwarded =
                query_velocity(&mut stream, &mut decoder, secret, config.read_timeout).await?;
            Profile {
                uuid: forwarded.uuid,
                name: forwarded.name,
                properties: forwarded.properties,
                address: Some(forwarded.address),
                chat_visibility: 0,
            }
        }
        forwarding::Forwarding::Bungee => {
            let legacy = legacy.context(
                "BungeeCord forwarding is enabled but the handshake has no forwarding data",
            )?;
            // BungeeCord's legacy payload omits the name; LoginStart supplies it,
            // exactly as Paper's createOfflineProfile does.
            Profile {
                uuid: legacy.uuid,
                name: start.name.clone(),
                properties: legacy.properties,
                address: Some(legacy.address),
                chat_visibility: 0,
            }
        }
    };
    send_packet(
        &mut stream,
        &SetCompression {
            threshold: VarInt(config.compression_threshold as i32),
        },
    )
    .await?;
    decoder.set_compression(Some(config.compression_threshold));
    let mut encoder = FrameEncoder::default();
    encoder.set_compression(Some(config.compression_threshold), 6);
    send_translated(
        &mut stream,
        &encoder,
        translator.as_mut(),
        State::Login,
        RawPacket::of(&LoginSuccess {
            uuid: profile.uuid,
            name: profile.name.clone(),
            properties: profile.properties.clone(),
            session_id: uuid::Uuid::new_v4(),
        }),
    )
    .await?;
    configure(stream, decoder, encoder, config, profile, translator).await
}

/// Ask Velocity for its modern-forwarding payload and verify the HMAC.
///
/// Paper sends `ClientboundCustomQueryPacket` on `velocity:player_info` with one
/// byte containing `MAX_SUPPORTED_FORWARDING_VERSION`; see
/// `ServerLoginPacketListenerImpl.java.patch` and `VelocityProxy.java`.
async fn query_velocity(
    stream: &mut Wire,
    decoder: &mut FrameDecoder,
    secret: &str,
    timeout: Duration,
) -> Result<forwarding::VelocityProfile> {
    use ferrite_protocol::packets::login::CustomQueryAnswer;

    let message_id = rand::random::<i32>();
    let query = forwarding::CustomQuery {
        message_id: VarInt(message_id),
        channel: forwarding::VELOCITY_PLAYER_INFO_CHANNEL.into(),
        data: ferrite_protocol::RestBytes(vec![
            u8::try_from(forwarding::VELOCITY_MAX_FORWARDING_VERSION)
                .context("forwarding version byte")?,
        ]),
    };
    send_packet(stream, &query)
        .await
        .context("send Velocity forwarding query")?;
    let packet = read_packet_timeout(stream, decoder, timeout).await?;
    let mut body = packet.as_slice();
    ensure!(
        VarInt::decode(&mut body)?.0 == CustomQueryAnswer::ID,
        "expected Velocity forwarding answer"
    );
    let answer = CustomQueryAnswer::decode(&mut body)?;
    ensure!(
        answer.message_id.0 == message_id,
        "unexpected Velocity forwarding message id"
    );
    let data = answer
        .data
        .context("client did not answer the Velocity forwarding query")?;
    forwarding::parse_velocity_payload(secret.as_bytes(), &data.0)
}
#[cfg(test)]
async fn send_compressed<P: Packet, W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    encoder: &FrameEncoder,
    packet: &P,
) -> Result<()> {
    let mut body = Vec::new();
    encode_packet(packet, &mut body);
    let mut wire = Vec::new();
    encoder.encode(&body, &mut wire);
    stream.write_all(&wire).await?;
    Ok(())
}
struct Profile {
    uuid: uuid::Uuid,
    name: String,
    properties: Vec<ferrite_protocol::packets::login::Property>,
    /// Address forwarded by the proxy, when one is configured.
    address: Option<std::net::IpAddr>,
    /// The client's chat visibility from configuration (`0` shown, `1` commands only, `2` hidden).
    chat_visibility: u8,
}
async fn configure(
    mut stream: Wire,
    mut decoder: FrameDecoder,
    encoder: FrameEncoder,
    config: ServerConfig,
    mut profile: Profile,
    mut translator: Option<Translator>,
) -> Result<()> {
    use ferrite_protocol::{
        Encode, RestBytes,
        packets::{
            configuration::{
                AcknowledgeFinishConfiguration, ClientboundCustomPayload, ClientboundKnownPacks,
                FinishConfiguration, KnownPack, RegistryData, ServerboundKnownPacks,
            },
            login::LoginAcknowledged,
        },
    };
    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != LoginAcknowledged::ID {
        bail!("expected login acknowledgement");
    }
    let mut brand = Vec::new();
    "Ferrite".encode(&mut brand);
    send_translated(
        &mut stream,
        &encoder,
        translator.as_mut(),
        State::Configuration,
        RawPacket::of(&ClientboundCustomPayload {
            channel: "minecraft:brand".into(),
            data: RestBytes(brand),
        }),
    )
    .await?;
    let pack = KnownPack {
        namespace: "minecraft".into(),
        id: "core".into(),
        version: ferrite_protocol::GAME_VERSION.into(),
    };
    send_translated(
        &mut stream,
        &encoder,
        translator.as_mut(),
        State::Configuration,
        RawPacket::of(&ClientboundKnownPacks {
            packs: vec![pack.clone()],
        }),
    )
    .await?;
    let mut timer = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(10),
    );
    loop {
        let packet = read_config_packet(
            &mut stream,
            &mut decoder,
            &encoder,
            &mut timer,
            Some(&mut profile),
            translator.as_mut(),
        )
        .await?;
        let mut body = packet.as_slice();
        let id = VarInt::decode(&mut body)?.0;
        if id == ServerboundKnownPacks::ID {
            if !ServerboundKnownPacks::decode(&mut body)?
                .packs
                .contains(&pack)
            {
                bail!("client does not know minecraft:core 26.3");
            }
            break;
        }
    }
    let vanilla = tokio::task::spawn_blocking(registry::load_vanilla).await??;
    for (name, entries) in &vanilla.registries {
        send_translated(
            &mut stream,
            &encoder,
            translator.as_mut(),
            State::Configuration,
            RawPacket::of(&RegistryData {
                registry: name.clone(),
                entries: entries.clone(),
            }),
        )
        .await?;
    }
    send_translated(
        &mut stream,
        &encoder,
        translator.as_mut(),
        State::Configuration,
        RawPacket::of(&vanilla.tags),
    )
    .await?;
    send_translated(
        &mut stream,
        &encoder,
        translator.as_mut(),
        State::Configuration,
        RawPacket::of(&FinishConfiguration),
    )
    .await?;
    loop {
        let packet = read_config_packet(
            &mut stream,
            &mut decoder,
            &encoder,
            &mut timer,
            Some(&mut profile),
            translator.as_mut(),
        )
        .await?;
        let mut body = packet.as_slice();
        if VarInt::decode(&mut body)?.0 == AcknowledgeFinishConfiguration::ID {
            break;
        }
    }
    if !config.online_mode && config.enforce_secure_profile {
        tracing::warn!(
            "enforce-secure-profile is ignored in offline mode: unsigned chat is accepted"
        );
    }
    play(
        stream, decoder, encoder, config, profile, vanilla, translator,
    )
    .await
}
async fn read_config_packet(
    stream: &mut Wire,
    decoder: &mut FrameDecoder,
    encoder: &FrameEncoder,
    timer: &mut tokio::time::Interval,
    mut profile: Option<&mut Profile>,
    mut translator: Option<&mut Translator>,
) -> Result<Vec<u8>> {
    use ferrite_protocol::packets::configuration::{
        ClientInformation, ClientboundKeepAlive, ServerboundCustomPayload, ServerboundKeepAlive,
    };
    loop {
        tokio::select! {
            _=timer.tick()=>{send_translated(stream,encoder,None,State::Configuration,RawPacket::of(&ClientboundKeepAlive{id:1})).await?;}
            packet=read_packet(stream,decoder)=>{
                let frame=packet?;
                let packets = if let Some(translator) = translator.as_deref_mut() {
                    translator.serverbound(State::Configuration, RawPacket::parse(&frame)?)?
                } else {
                    vec![RawPacket::parse(&frame)?]
                };
                for packet in packets {
                    let mut body = packet.body.as_slice();
                    match packet.id {
                        0 => { let info = ClientInformation::decode(&mut body)?; if let Some(profile)=profile.as_deref_mut(){settings::record(profile.uuid,settings::ClientSettings{locale:info.locale.clone(),view_distance:info.view_distance,left_handed:info.main_hand.0==0});profile.chat_visibility=match info.chat_mode.0{1=>1,2=>2,_=>0};} },
                        2 => { ServerboundCustomPayload::decode(&mut body)?; },
                        4 => { ServerboundKeepAlive::decode(&mut body)?; },
                        _ => return Ok(packet.to_frame()),
                    }
                }
            }
        }
    }
}
#[cfg(test)]
fn flat_chunk(x: i32, z: i32, biome: i32) -> ferrite_protocol::packets::play::ChunkData {
    use ferrite_protocol::{
        Encode,
        packets::play::{ChunkData, Heightmap},
    };
    let mut sections = Vec::new();
    for section in 0..24 {
        let platform = section == 8;
        (if platform { 256i16 } else { 0i16 }).encode(&mut sections);
        0i16.encode(&mut sections);
        if platform {
            4u8.encode(&mut sections);
            VarInt(2).encode(&mut sections);
            VarInt(0).encode(&mut sections);
            VarInt(1).encode(&mut sections);
            for i in 0..256 {
                (if i < 16 { 0x1111_1111_1111_1111i64 } else { 0 }).encode(&mut sections);
            }
        } else {
            0u8.encode(&mut sections);
            VarInt(0).encode(&mut sections);
        }
        0u8.encode(&mut sections);
        VarInt(biome).encode(&mut sections);
    }
    let mut heights = vec![0i64; 37];
    for i in 0..256 {
        heights[i / 7] |= 129i64 << ((i % 7) * 9);
    }
    ChunkData {
        x,
        z,
        heightmaps: vec![
            Heightmap {
                kind: VarInt(1),
                data: heights.clone(),
            },
            Heightmap {
                kind: VarInt(4),
                data: heights,
            },
        ],
        data: sections,
        sky_mask: vec![],
        block_mask: vec![],
        empty_sky_mask: vec![(1 << 26) - 1],
        empty_block_mask: vec![(1 << 26) - 1],
        sky_arrays: vec![],
        block_arrays: vec![],
    }
}
async fn play(
    stream: Wire,
    mut decoder: FrameDecoder,
    encoder: FrameEncoder,
    config: ServerConfig,
    profile: Profile,
    vanilla: registry::VanillaData,
    translator: Option<Translator>,
) -> Result<()> {
    use game::GameMessage;
    tracing::info!(name = %profile.name, address = ?profile.address, "entering play");
    let game = config.game.context("game loop unavailable")?;
    let dimension_type = vanilla.registries["minecraft:dimension_type"]
        .iter()
        .position(|v| v == "minecraft:overworld")
        .context("missing overworld")? as i32;
    let biome = vanilla.registries["minecraft:worldgen/biome"]
        .iter()
        .position(|v| v == "minecraft:plains")
        .context("missing plains")? as i32;
    let chat_type = vanilla.registries["minecraft:chat_type"]
        .iter()
        .position(|v| v == "minecraft:chat")
        .context("missing chat type")? as i32;
    static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let session = NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let uuid = profile.uuid;
    let status = config.status.clone();
    let status_name = profile.name.clone();
    let (outbound, queue) = game::OutboundSender::channel(config.send_queue_bytes);
    game.send(GameMessage::Joined {
        session,
        uuid,
        name: profile.name,
        properties: profile.properties,
        address: profile.address,
        dimension_type,
        biome,
        view_distance: config.view_distance.clamp(2, 32),
        online_mode: config.online_mode,
        // Vanilla: enforce-secure-profile only applies to authenticated servers.
        secure_chat: config.enforce_secure_profile && config.online_mode,
        chat_type,
        chat_visibility: profile.chat_visibility,
        max_players: config.max_players,
        outbound: outbound.clone(),
    })
    .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
    status.join(session, status_name, uuid);
    // Ticks only push already-encoded bytes into the bounded queue. This task owns the socket
    // write side and does compression, encryption and one write per batch, so a slow client
    // can never stall a tick.
    let (mut reader, writer) = stream.into_split();
    let translator =
        translator.map(|translator| std::sync::Arc::new(std::sync::Mutex::new(translator)));
    let mut writer_task = tokio::spawn(write_loop(
        writer,
        encoder,
        queue,
        config.write_timeout,
        translator.clone(),
    ));
    let result = async {
        loop {
            tokio::select! {
                finished = &mut writer_task => {
                    return match finished {
                        Ok(result) => result,
                        Err(error) => Err(anyhow::anyhow!("writer task failed: {error}")),
                    };
                }
                frame = read_packet(&mut reader, &mut decoder) => {
                    let frame = frame?;
                    match &translator {
                        // Native clients decode the frame directly, as before translation.
                        None => {
                            if let Some(packet) = game::decode_play(&frame)? {
                                game.send(GameMessage::Input { session, uuid, packet })
                                    .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
                            }
                        }
                        Some(translator) => {
                            let packets = translator
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .serverbound(State::Play, RawPacket::parse(&frame)?)?;
                            for packet in packets {
                                if let Some(packet) = game::decode_play(&packet.to_frame())? {
                                    game.send(GameMessage::Input { session, uuid, packet })
                                        .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    .await;
    writer_task.abort();
    let _ = game.send(GameMessage::Left { session, uuid });
    status.leave(session);
    result
}
async fn write_loop<W: wire::BatchWriter>(
    mut writer: W,
    encoder: FrameEncoder,
    mut queue: game::OutboundReceiver,
    write_timeout: std::time::Duration,
    translator: Option<std::sync::Arc<std::sync::Mutex<Translator>>>,
) -> Result<()> {
    use ferrite_protocol::{
        packets::play::{PlayDisconnect, PlayKeepAlive},
        types::PlainText,
    };
    let mut keepalive_body = Vec::new();
    encode_packet(&PlayKeepAlive { id: 1 }, &mut keepalive_body);
    let mut keepalive = tokio::time::interval_at(
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(10),
    );
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let mut batch = Vec::new();
        let mut close = None;
        tokio::select! {
            biased;
            _ = keepalive.tick() => {
                if translator.is_none() {
                    encoder.encode(&keepalive_body, &mut batch);
                } else {
                    queue_raw(
                        &mut batch,
                        &encoder,
                        &translator,
                        RawPacket::of(&PlayKeepAlive { id: 1 }),
                    );
                }
            }
            incoming = queue.next() => match incoming {
                game::Incoming::Overflow => {
                    queue.discard();
                    close = Some("Timed out".to_string());
                }
                game::Incoming::Closed => return Ok(()),
                game::Incoming::Message(msg) => {
                    queue_batch(&mut batch, &encoder, &translator, msg, &mut close);
                    while close.is_none()
                        && let Some(msg) = queue.try_recv()
                    {
                        queue_batch(&mut batch, &encoder, &translator, msg, &mut close);
                    }
                }
            }
        }
        // Overflow can latch after the last `try_recv`; drop the rest of the backlog and send
        // the disconnect instead of flushing packets the client is not reading.
        if close.is_none() && queue.overflowed() {
            queue.discard();
            close = Some("Timed out".to_string());
        }
        if let Some(reason) = close {
            if translator.is_none() {
                let mut body = Vec::new();
                encode_packet(
                    &PlayDisconnect {
                        reason: PlainText(reason),
                    },
                    &mut body,
                );
                encoder.encode(&body, &mut batch);
            } else {
                queue_raw(
                    &mut batch,
                    &encoder,
                    &translator,
                    RawPacket::of(&PlayDisconnect {
                        reason: PlainText(reason),
                    }),
                );
            }
            if !batch.is_empty() {
                write_batch(&mut writer, &mut batch, write_timeout).await?;
            }
            return Ok(());
        }
        if !batch.is_empty() {
            write_batch(&mut writer, &mut batch, write_timeout).await?;
        }
    }
}
fn queue_batch(
    batch: &mut Vec<u8>,
    encoder: &FrameEncoder,
    translator: &Option<std::sync::Arc<std::sync::Mutex<Translator>>>,
    msg: game::Outbound,
    close: &mut Option<String>,
) {
    match msg {
        game::Outbound::Packet(body) => {
            if translator.is_none() {
                // Native clients: the bytes are already in the client's protocol.
                encoder.encode(&body, batch);
                return;
            }
            match RawPacket::parse(&body) {
                Ok(packet) => queue_raw(batch, encoder, translator, packet),
                Err(error) => {
                    tracing::error!(%error, "invalid core packet");
                    *close = Some("Internal error".into());
                }
            }
        }
        game::Outbound::Disconnect(reason) => *close = Some(reason),
    }
}

/// Encode one core packet into the client's protocol and append it to the batch. Native
/// clients take the exact same path as before translation existed.
fn queue_raw(
    batch: &mut Vec<u8>,
    encoder: &FrameEncoder,
    translator: &Option<std::sync::Arc<std::sync::Mutex<Translator>>>,
    packet: RawPacket,
) {
    match translator {
        Some(translator) => {
            let raw = packet.to_frame();
            match translator
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clientbound(State::Play, packet)
            {
                Ok(packets) => {
                    for packet in packets {
                        encoder.encode(&packet.to_frame(), batch);
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "packet translation failed");
                    encoder.encode(&raw, batch);
                }
            }
        }
        None => encoder.encode(&packet.to_frame(), batch),
    }
}
async fn write_batch<W: wire::BatchWriter>(
    writer: &mut W,
    batch: &mut [u8],
    write_timeout: std::time::Duration,
) -> Result<()> {
    match tokio::time::timeout(write_timeout, wire::BatchWriter::write_batch(writer, batch)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => bail!("client timed out while writing"),
    }
}
fn minecraft_digest(digest: &[u8]) -> String {
    num_bigint::BigInt::from_signed_bytes_be(digest).to_str_radix(16)
}
async fn authenticate(
    stream: &mut Wire,
    decoder: &mut FrameDecoder,
    key: &rsa::RsaPrivateKey,
    name: &str,
    online: bool,
    timeout: Duration,
) -> Result<(
    uuid::Uuid,
    String,
    Vec<ferrite_protocol::packets::login::Property>,
)> {
    use ferrite_protocol::packets::login::{EncryptionRequest, EncryptionResponse, Property};
    use rand::RngCore;
    use rsa::{Pkcs1v15Encrypt, pkcs8::EncodePublicKey};
    use sha1::{Digest, Sha1};
    let der = rsa::RsaPublicKey::from(key)
        .to_public_key_der()?
        .as_bytes()
        .to_vec();
    let mut token = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut token);
    send_packet(
        stream,
        &EncryptionRequest {
            server_id: String::new(),
            public_key: der.clone(),
            verify_token: token.to_vec(),
            should_authenticate: online,
        },
    )
    .await?;
    // Do not read ahead across the encryption boundary.
    let response = tokio::time::timeout(timeout, async {
        loop {
            if let Some(frame) = decoder.next_frame()? {
                return Ok::<_, anyhow::Error>(frame);
            }
            let mut byte = [0u8; 1];
            let n = stream.read(&mut byte).await?;
            if n == 0 {
                bail!("connection closed during encryption");
            }
            decoder.feed(&byte);
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for the encryption response"))??;
    let mut body = response.as_slice();
    if VarInt::decode(&mut body)?.0 != EncryptionResponse::ID {
        bail!("expected encryption response");
    }
    let response = EncryptionResponse::decode(&mut body)?;
    let secret = key
        .decrypt(Pkcs1v15Encrypt, &response.shared_secret)
        .context("invalid encrypted secret")?;
    let check = key
        .decrypt(Pkcs1v15Encrypt, &response.verify_token)
        .context("invalid encrypted token")?;
    if check != token {
        bail!("invalid verify token");
    }
    let secret: [u8; 16] = secret
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid secret length"))?;
    stream.enable_encryption(&secret);
    if !online {
        return Ok((offline_uuid(name), name.to_owned(), vec![]));
    }
    let mut hash = Sha1::new();
    hash.update(secret);
    hash.update(&der);
    let hash = minecraft_digest(&hash.finalize());
    let response = reqwest::Client::new()
        .get("https://sessionserver.mojang.com/session/minecraft/hasJoined")
        .query(&[("username", name), ("serverId", hash.as_str())])
        .send()
        .await?
        .error_for_status()?;
    let profile: serde_json::Value = response
        .json()
        .await
        .context("session authentication rejected")?;
    let uuid = uuid::Uuid::parse_str(profile["id"].as_str().context("session UUID")?)?;
    let authenticated = profile["name"].as_str().context("session name")?;
    if !authenticated.eq_ignore_ascii_case(name) {
        bail!("session name mismatch");
    }
    let mut properties = Vec::new();
    if let Some(values) = profile["properties"].as_array() {
        for value in values {
            properties.push(Property {
                name: value["name"].as_str().context("property name")?.into(),
                value: value["value"].as_str().context("property value")?.into(),
                signature: value["signature"].as_str().map(str::to_owned),
            });
        }
    }
    Ok((uuid, authenticated.into(), properties))
}
fn valid_username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 16
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
/// The UUID an offline-mode server gives a player name (`OfflinePlayer:<name>`, MD5, version 3).
pub fn offline_uuid(name: &str) -> uuid::Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{name}"));
    let mut bytes = digest.0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::{
        Decode, Encode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
        packets::{
            handshake::{Intent, Intention},
            status::{PingRequest, PongResponse, StatusRequest, StatusResponse},
        },
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A writer that blocks inside its first batch until the test releases it, then records
    /// every byte. This stalls the connection without relying on a socket's buffer sizes.
    struct StalledWriter {
        entered: std::sync::Arc<tokio::sync::Barrier>,
        release: Option<tokio::sync::oneshot::Receiver<()>>,
        stalled: bool,
        written: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl crate::wire::BatchWriter for StalledWriter {
        async fn write_batch(&mut self, batch: &mut [u8]) -> std::io::Result<()> {
            if !self.stalled {
                self.stalled = true;
                self.entered.wait().await;
                if let Some(release) = self.release.take() {
                    let _ = release.await;
                }
            }
            self.written
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend_from_slice(batch);
            Ok(())
        }
    }

    /// The byte budget, not a kernel socket buffer, decides when a connection is too slow:
    /// while one batch write is stalled, further packets exhaust the queue budget, and the
    /// writer must discard the backlog and emit the exact `"Timed out"` play disconnect.
    #[tokio::test]
    async fn stalled_writer_overflows_budget_and_sends_timed_out_disconnect() {
        use ferrite_protocol::packets::play::PlayDisconnect;

        let (tx, rx) = game::OutboundSender::channel(16);
        let entered = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let (release, release_rx) = tokio::sync::oneshot::channel();
        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = StalledWriter {
            entered: std::sync::Arc::clone(&entered),
            release: Some(release_rx),
            stalled: false,
            written: std::sync::Arc::clone(&written),
        };
        let task = tokio::spawn(write_loop(
            writer,
            FrameEncoder::default(),
            rx,
            Duration::from_secs(5),
            None,
        ));

        // The first packet makes the writer enter its blocked batch write.
        let packet = || game::Outbound::Packet(vec![0u8; 8].into());
        tx.send(packet()).unwrap();
        entered.wait().await;

        // Nothing can be drained, so two packets fill the queue and a third exceeds the
        // 16-byte budget; the overflow latches and drops the packet.
        let queued = || game::Outbound::Packet(vec![1u8; 8].into());
        tx.send(queued()).unwrap();
        tx.send(queued()).unwrap();
        assert_eq!(tx.send(queued()).unwrap_err(), game::SendError::Full);
        assert!(tx.is_overflowed());

        release.send(()).unwrap();
        task.await.unwrap().unwrap();

        let bytes = written.lock().unwrap().clone();
        let mut decoder = FrameDecoder::default();
        decoder.feed(&bytes);
        let frames: Vec<Vec<u8>> = std::iter::from_fn(|| decoder.next_frame().unwrap()).collect();
        let mut last = frames.last().expect("writer sent no packets").as_slice();
        assert_eq!(VarInt::decode(&mut last).unwrap().0, PlayDisconnect::ID);
        assert_eq!(
            PlayDisconnect::decode(&mut last).unwrap().reason.0,
            "Timed out"
        );
    }

    #[tokio::test]
    async fn status_ping_over_tcp() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, ServerConfig::default()));
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut wire = Vec::new();
        for packet in [
            {
                let mut b = Vec::new();
                encode_packet(
                    &Intention {
                        protocol_version: 777,
                        server_address: "localhost".into(),
                        server_port: addr.port(),
                        intent: Intent::Status,
                    },
                    &mut b,
                );
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(&StatusRequest, &mut b);
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(&PingRequest { payload: 42 }, &mut b);
                b
            },
        ] {
            FrameEncoder::default().encode(&packet, &mut wire);
        }
        socket.write_all(&wire).await.unwrap();
        let mut decoder = FrameDecoder::default();
        let mut frames = Vec::new();
        while frames.len() < 2 {
            let mut buf = [0; 1024];
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), socket.read(&mut buf))
                .await
                .unwrap()
                .unwrap();
            assert_ne!(n, 0);
            decoder.feed(&buf[..n]);
            while let Some(frame) = decoder.next_frame().unwrap() {
                frames.push(frame);
            }
        }
        let mut body = frames[0].as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, StatusResponse::ID);
        let response: serde_json::Value =
            serde_json::from_str(&StatusResponse::decode(&mut body).unwrap().json).unwrap();
        assert_eq!(response["version"]["protocol"], 777);
        assert_eq!(response["version"]["name"], "26.3");
        assert_eq!(response["players"]["max"], 20);
        assert_eq!(response["players"]["online"], 0);
        assert_eq!(response["players"]["sample"], serde_json::json!([]));
        assert_eq!(response["description"]["text"], "A Ferrite server");
        assert_eq!(response["enforcesSecureChat"], true);
        assert!(response.get("favicon").is_none());
        let mut body = frames[1].as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, PongResponse::ID);
        assert_eq!(PongResponse::decode(&mut body).unwrap().payload, 42);
        server.abort();
    }

    #[test]
    fn status_json_reports_sample_favicon_and_secure_chat() {
        let mut config = ServerConfig {
            motd: "Hello".into(),
            max_players: 100,
            favicon: Some("data:image/png;base64,AA==".into()),
            ..Default::default()
        };
        for index in 0..13u128 {
            config.status.join(
                index as u64 + 1,
                format!("Player{index}"),
                uuid::Uuid::from_u128(index + 1),
            );
        }
        let status: serde_json::Value = serde_json::from_str(&status_json(&config)).unwrap();
        assert_eq!(status["version"]["name"], ferrite_protocol::GAME_VERSION);
        assert_eq!(
            status["version"]["protocol"],
            ferrite_protocol::PROTOCOL_VERSION
        );
        assert_eq!(status["players"]["max"], 100);
        assert_eq!(status["players"]["online"], 13);
        let sample = status["players"]["sample"].as_array().unwrap();
        assert_eq!(sample.len(), STATUS_SAMPLE_LIMIT);
        assert_eq!(sample[0]["name"], "Player0");
        assert_eq!(sample[0]["id"], uuid::Uuid::from_u128(1).to_string());
        assert_eq!(sample[11]["name"], "Player11");
        assert_eq!(status["description"]["text"], "Hello");
        assert_eq!(status["favicon"], "data:image/png;base64,AA==");
        assert_eq!(status["enforcesSecureChat"], true);

        // Offline mode must not advertise secure-chat enforcement.
        config.online_mode = false;
        let offline: serde_json::Value = serde_json::from_str(&status_json(&config)).unwrap();
        assert_eq!(offline["enforcesSecureChat"], false);
    }

    #[test]
    fn status_state_scopes_leave_to_the_session() {
        let status = StatusState::default();
        let alice = uuid::Uuid::from_u128(1);
        let bob = uuid::Uuid::from_u128(2);
        status.join(1, "Alice", alice);
        status.join(2, "Bob", bob);
        assert_eq!(status.online(), 2);
        // A duplicate login replaces the entry, keeping one row per UUID.
        status.join(3, "Alice", alice);
        assert_eq!(status.online(), 2);
        // The displaced connection's leave must not remove the new session.
        status.leave(1);
        assert_eq!(status.online(), 2);
        status.leave(3);
        assert_eq!(
            status.players(),
            vec![SamplePlayer {
                name: "Bob".into(),
                id: bob
            }]
        );
    }

    #[tokio::test]
    async fn silent_client_is_disconnected_by_the_read_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                read_timeout: Duration::from_millis(150),
                ..Default::default()
            },
        ));
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut byte))
            .await
            .expect("server did not close the idle connection")
            .unwrap();
        assert_eq!(read, 0, "expected EOF after the read timeout");
        server.abort();
    }

    #[tokio::test]
    async fn oversized_handshake_frame_is_rejected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                ..Default::default()
            },
        ));
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut header = Vec::new();
        VarInt(ferrite_protocol::frame::MAX_FRAME_LEN as i32 + 1).encode(&mut header);
        socket.write_all(&header).await.unwrap();
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut byte))
            .await
            .expect("server did not reject the oversized frame")
            .unwrap();
        assert_eq!(read, 0, "expected EOF after rejecting the frame");
        server.abort();
    }
}
#[cfg(test)]
mod login_test {
    use super::*;
    use ferrite_protocol::packets::login::{LoginStart, LoginSuccess, SetCompression};
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    #[tokio::test]
    async fn offline_login_compresses_and_sends_profile() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                ..Default::default()
            },
        ));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut wire = Vec::new();
        for body in [
            {
                let mut b = Vec::new();
                encode_packet(
                    &Intention {
                        protocol_version: 777,
                        server_address: "localhost".into(),
                        server_port: addr.port(),
                        intent: Intent::Login,
                    },
                    &mut b,
                );
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(
                    &LoginStart {
                        name: "Steve".into(),
                        uuid: uuid::Uuid::nil(),
                    },
                    &mut b,
                );
                b
            },
        ] {
            FrameEncoder::default().encode(&body, &mut wire);
        }
        stream.write_all(&wire).await.unwrap();
        let mut decoder = FrameDecoder::default();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, SetCompression::ID);
        assert_eq!(SetCompression::decode(&mut body).unwrap().threshold.0, 256);
        decoder.set_compression(Some(256));
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, LoginSuccess::ID);
        let profile = LoginSuccess::decode(&mut body).unwrap();
        assert_eq!(profile.name, "Steve");
        assert_eq!(
            profile.uuid.to_string(),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
        server.abort();
    }

    #[tokio::test]
    async fn velocity_forwarding_replaces_the_login_profile() {
        use ferrite_protocol::packets::login::CustomQueryAnswer;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let secret = "hunter2";
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                forwarding: forwarding::Forwarding::Velocity {
                    secret: secret.into(),
                },
                ..Default::default()
            },
        ));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut wire = Vec::new();
        for body in [
            {
                let mut b = Vec::new();
                encode_packet(
                    &Intention {
                        protocol_version: 777,
                        server_address: "localhost".into(),
                        server_port: addr.port(),
                        intent: Intent::Login,
                    },
                    &mut b,
                );
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(
                    &LoginStart {
                        name: "Steve".into(),
                        uuid: uuid::Uuid::nil(),
                    },
                    &mut b,
                );
                b
            },
        ] {
            FrameEncoder::default().encode(&body, &mut wire);
        }
        stream.write_all(&wire).await.unwrap();
        let mut decoder = FrameDecoder::default();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(
            VarInt::decode(&mut body).unwrap().0,
            forwarding::CustomQuery::ID
        );
        let query = forwarding::CustomQuery::decode(&mut body).unwrap();
        assert_eq!(query.channel, forwarding::VELOCITY_PLAYER_INFO_CHANNEL);
        assert_eq!(query.data.0, vec![4]);
        let uuid = uuid::Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        let payload = forwarding::signed_velocity_payload(
            secret.as_bytes(),
            4,
            "203.0.113.7",
            uuid,
            "Notch",
            &[],
        );
        send_packet(
            &mut stream,
            &CustomQueryAnswer {
                message_id: query.message_id,
                data: Some(ferrite_protocol::RestBytes(payload)),
            },
        )
        .await
        .unwrap();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, SetCompression::ID);
        decoder.set_compression(Some(256));
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, LoginSuccess::ID);
        let profile = LoginSuccess::decode(&mut body).unwrap();
        assert_eq!(profile.uuid, uuid);
        assert_eq!(profile.name, "Notch");
        server.abort();
    }

    /// Send a handshake and a login start, then return the first reply frame.
    async fn login_attempt(
        addr: std::net::SocketAddr,
        protocol: i32,
    ) -> (TcpStream, FrameDecoder, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut wire = Vec::new();
        for body in [
            {
                let mut b = Vec::new();
                encode_packet(
                    &Intention {
                        protocol_version: protocol,
                        server_address: "localhost".into(),
                        server_port: addr.port(),
                        intent: Intent::Login,
                    },
                    &mut b,
                );
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(
                    &LoginStart {
                        name: "Steve".into(),
                        uuid: uuid::Uuid::nil(),
                    },
                    &mut b,
                );
                b
            },
        ] {
            FrameEncoder::default().encode(&body, &mut wire);
        }
        stream.write_all(&wire).await.unwrap();
        let mut decoder = FrameDecoder::default();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .expect("server did not answer the login attempt")
        .unwrap();
        (stream, decoder, packet)
    }

    #[tokio::test]
    async fn native_only_rejects_older_protocols() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                supported_versions: SupportedVersions::Native,
                ..Default::default()
            },
        ));
        let (_stream, _decoder, packet) = login_attempt(addr, 776).await;
        let mut body = packet.as_slice();
        assert_eq!(
            VarInt::decode(&mut body).unwrap().0,
            0,
            "expected the login-state disconnect"
        );
        let reason = String::decode(&mut body).unwrap();
        assert!(
            reason.contains("multiplayer.disconnect.outdated_client"),
            "{reason}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn older_protocol_is_translated_when_all_versions_are_supported() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                ..Default::default()
            },
        ));
        // 26.2 (776): the login flow is reached instead of the "outdated client" disconnect.
        let (mut stream, mut decoder, packet) = login_attempt(addr, 776).await;
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, SetCompression::ID);
        decoder.set_compression(Some(256));
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, LoginSuccess::ID);
        assert_eq!(LoginSuccess::decode(&mut body).unwrap().name, "Steve");

        // A protocol outside the chain is rejected even though older versions are accepted.
        let (_stream, _decoder, packet) = login_attempt(addr, 775).await;
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, 0);
        let reason = String::decode(&mut body).unwrap();
        assert!(
            reason.contains("multiplayer.disconnect.outdated_client"),
            "{reason}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn status_advertises_the_older_protocol() {
        use ferrite_protocol::packets::status::{StatusRequest, StatusResponse};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, ServerConfig::default()));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut wire = Vec::new();
        for body in [
            {
                let mut b = Vec::new();
                encode_packet(
                    &Intention {
                        protocol_version: 776,
                        server_address: "localhost".into(),
                        server_port: addr.port(),
                        intent: Intent::Status,
                    },
                    &mut b,
                );
                b
            },
            {
                let mut b = Vec::new();
                encode_packet(&StatusRequest, &mut b);
                b
            },
        ] {
            FrameEncoder::default().encode(&body, &mut wire);
        }
        stream.write_all(&wire).await.unwrap();
        let mut decoder = FrameDecoder::default();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, StatusResponse::ID);
        let response: serde_json::Value =
            serde_json::from_str(&StatusResponse::decode(&mut body).unwrap().json).unwrap();
        assert_eq!(response["version"]["protocol"], 776);
        server.abort();
    }
}

#[cfg(test)]
mod registry_tests {
    #[test]
    fn vanilla_registries_and_tags_are_resolved() {
        let data = super::registry::load_vanilla().unwrap();
        let dimension = &data.registries["minecraft:dimension_type"];
        assert!(dimension.contains(&"minecraft:overworld".to_string()));
        let damage = data
            .tags
            .registries
            .iter()
            .find(|r| r.registry == "minecraft:damage_type")
            .unwrap();
        let tag = damage
            .tags
            .iter()
            .find(|t| t.name == "minecraft:is_fire")
            .unwrap();
        let damage_entries = &data.registries["minecraft:damage_type"];
        assert!(
            tag.entries
                .iter()
                .any(|id| damage_entries[id.0 as usize] == "minecraft:lava")
        );
    }
}
mod registry;
#[cfg(test)]
mod configuration_test {
    use super::*;
    use ferrite_protocol::packets::{
        configuration::{
            ClientboundKnownPacks, FinishConfiguration, KnownPack, RegistryData,
            ServerboundKnownPacks,
        },
        login::{LoginAcknowledged, LoginStart},
    };
    use tokio::net::TcpStream;
    #[tokio::test]
    async fn configuration_sends_known_pack_and_registries() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (game_tx, mut game_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            if let Some(game::GameMessage::Joined {
                outbound,
                dimension_type,
                biome,
                ..
            }) = game_rx.recv().await
            {
                use ferrite_protocol::packets::play::*;
                let login = PlayLogin {
                    entity_id: 1,
                    hardcore: false,
                    dimensions: vec!["minecraft:overworld".into()],
                    max_players: VarInt(20),
                    view_distance: VarInt(2),
                    simulation_distance: VarInt(2),
                    reduced_debug: false,
                    respawn_screen: true,
                    limited_crafting: false,
                    dimension_type: VarInt(dimension_type),
                    dimension: "minecraft:overworld".into(),
                    seed: 0,
                    game_mode: VarInt(0),
                    previous_game_mode: VarInt(0),
                    debug: false,
                    flat: true,
                    death_location: None,
                    portal_cooldown: VarInt(0),
                    sea_level: VarInt(63),
                    online_mode: false,
                    secure_chat: false,
                };
                let _ = outbound.send(game::packet(&login));
                let _ = outbound.send(game::packet(&PositionSync {
                    teleport_id: VarInt(1),
                    x: 0.5,
                    y: 65.0,
                    z: 0.5,
                    velocity_x: 0.0,
                    velocity_y: 0.0,
                    velocity_z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    flags: 0,
                }));
                let _ = outbound.send(game::packet(&GameEvent {
                    event: 13,
                    value: 0.0,
                }));
                let _ = outbound.send(game::packet(&flat_chunk(0, 0, biome)));
            }
        });
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                game: Some(game_tx),
                ..Default::default()
            },
        ));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut encoder = FrameEncoder::default();
        let mut decoder = FrameDecoder::default();
        send_packet(
            &mut stream,
            &Intention {
                protocol_version: 777,
                server_address: "localhost".into(),
                server_port: addr.port(),
                intent: Intent::Login,
            },
        )
        .await
        .unwrap();
        send_packet(
            &mut stream,
            &LoginStart {
                name: "Steve".into(),
                uuid: uuid::Uuid::nil(),
            },
        )
        .await
        .unwrap();
        read_packet(&mut stream, &mut decoder).await.unwrap();
        encoder.set_compression(Some(256), 6);
        decoder.set_compression(Some(256));
        read_packet(&mut stream, &mut decoder).await.unwrap();
        send_compressed(&mut stream, &encoder, &LoginAcknowledged)
            .await
            .unwrap();
        let mut saw_pack = false;
        while !saw_pack {
            let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
            let mut body = bytes.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            if id == ClientboundKnownPacks::ID {
                let p = ClientboundKnownPacks::decode(&mut body).unwrap();
                assert_eq!(p.packs[0].version, "26.3");
                saw_pack = true;
            }
        }
        send_compressed(
            &mut stream,
            &encoder,
            &ServerboundKnownPacks {
                packs: vec![KnownPack {
                    namespace: "minecraft".into(),
                    id: "core".into(),
                    version: "26.3".into(),
                }],
            },
        )
        .await
        .unwrap();
        let mut found = false;
        loop {
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                read_packet(&mut stream, &mut decoder),
            )
            .await
            .unwrap()
            .unwrap();
            let mut body = bytes.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            if id == RegistryData::ID {
                let r = RegistryData::decode(&mut body).unwrap();
                if r.registry == "minecraft:dimension_type" {
                    assert!(r.entries.contains(&"minecraft:overworld".to_string()));
                    found = true;
                }
            }
            if id == FinishConfiguration::ID {
                break;
            }
        }
        assert!(found);
        send_compressed(
            &mut stream,
            &encoder,
            &ferrite_protocol::packets::configuration::AcknowledgeFinishConfiguration,
        )
        .await
        .unwrap();
        let mut login = false;
        let mut position = false;
        let mut event = false;
        let mut chunk = false;
        while !(login && position && event && chunk) {
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                read_packet(&mut stream, &mut decoder),
            )
            .await
            .unwrap()
            .unwrap();
            let mut body = bytes.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            match id {
                50 => {
                    let p = ferrite_protocol::packets::play::PlayLogin::decode(&mut body).unwrap();
                    assert_eq!(p.dimension, "minecraft:overworld");
                    login = true;
                }
                73 => {
                    let p =
                        ferrite_protocol::packets::play::PositionSync::decode(&mut body).unwrap();
                    assert_eq!((p.x, p.y, p.z), (0.5, 65.0, 0.5));
                    send_compressed(
                        &mut stream,
                        &encoder,
                        &ferrite_protocol::packets::play::ConfirmTeleport {
                            id: p.teleport_id,
                            x: p.x,
                            y: p.y,
                            z: p.z,
                            yaw: p.yaw,
                            pitch: p.pitch,
                        },
                    )
                    .await
                    .unwrap();
                    position = true;
                }
                39 => {
                    let p = ferrite_protocol::packets::play::GameEvent::decode(&mut body).unwrap();
                    event = p.event == 13;
                }
                46 => {
                    let p = ferrite_protocol::packets::play::ChunkData::decode(&mut body).unwrap();
                    assert!(!p.data.is_empty());
                    assert_eq!(p.heightmaps.len(), 2);
                    chunk = true;
                }
                _ => {}
            }
        }
        server.abort();
    }
}

#[cfg(test)]
mod encryption_tests {
    use super::*;
    use ferrite_protocol::packets::login::{
        EncryptionRequest, EncryptionResponse, LoginStart, LoginSuccess, SetCompression,
    };
    use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
    use sha1::{Digest, Sha1};
    use tokio::net::TcpStream;
    #[test]
    fn minecraft_digest_vectors() {
        for (input, expected) in [
            ("Notch", "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"),
            ("jeb_", "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"),
            ("simon", "88e16a1019277b15d58faf0541e11910eb756f6"),
        ] {
            assert_eq!(minecraft_digest(&Sha1::digest(input.as_bytes())), expected);
        }
    }
    #[tokio::test]
    async fn encrypted_login_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                encrypt_offline: true,
                ..Default::default()
            },
        ));
        let mut stream = Wire::new(TcpStream::connect(addr).await.unwrap());
        let mut decoder = FrameDecoder::default();
        send_packet(
            &mut stream,
            &Intention {
                protocol_version: 777,
                server_address: "localhost".into(),
                server_port: addr.port(),
                intent: Intent::Login,
            },
        )
        .await
        .unwrap();
        send_packet(
            &mut stream,
            &LoginStart {
                name: "Steve".into(),
                uuid: uuid::Uuid::nil(),
            },
        )
        .await
        .unwrap();
        let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
        let mut body = bytes.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, EncryptionRequest::ID);
        let request = EncryptionRequest::decode(&mut body).unwrap();
        assert!(!request.should_authenticate);
        let key = RsaPublicKey::from_public_key_der(&request.public_key).unwrap();
        let secret = [42u8; 16];
        let mut rng = rand::thread_rng();
        send_packet(
            &mut stream,
            &EncryptionResponse {
                shared_secret: key.encrypt(&mut rng, Pkcs1v15Encrypt, &secret).unwrap(),
                verify_token: key
                    .encrypt(&mut rng, Pkcs1v15Encrypt, &request.verify_token)
                    .unwrap(),
            },
        )
        .await
        .unwrap();
        stream.enable_encryption(&secret);
        let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
        let mut body = bytes.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, SetCompression::ID);
        decoder.set_compression(Some(256));
        let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
        let mut body = bytes.as_slice();
        assert_eq!(VarInt::decode(&mut body).unwrap().0, LoginSuccess::ID);
        assert_eq!(LoginSuccess::decode(&mut body).unwrap().name, "Steve");
        let mut encoder = FrameEncoder::default();
        encoder.set_compression(Some(256), 6);
        send_compressed(
            &mut stream,
            &encoder,
            &ferrite_protocol::packets::login::LoginAcknowledged,
        )
        .await
        .unwrap();
        let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
        let mut body = bytes.as_slice();
        assert_eq!(
            VarInt::decode(&mut body).unwrap().0,
            ferrite_protocol::packets::configuration::ClientboundCustomPayload::ID
        );
        assert_eq!(
            ferrite_protocol::packets::configuration::ClientboundCustomPayload::decode(&mut body)
                .unwrap()
                .channel,
            "minecraft:brand"
        );
        server.abort();
    }
}

#[cfg(test)]
mod chunk_tests {
    use super::*;
    #[test]
    fn chunk_contains_platform_and_correct_heightmap() {
        let chunk = flat_chunk(0, 0, 5);
        let mut input = chunk.data.as_slice();
        for section in 0..24 {
            assert_eq!(
                i16::decode(&mut input).unwrap(),
                if section == 8 { 256 } else { 0 }
            );
            assert_eq!(i16::decode(&mut input).unwrap(), 0);
            let bits = u8::decode(&mut input).unwrap();
            if section == 8 {
                assert_eq!(bits, 4);
                assert_eq!(VarInt::decode(&mut input).unwrap().0, 2);
                assert_eq!(VarInt::decode(&mut input).unwrap().0, 0);
                assert_eq!(VarInt::decode(&mut input).unwrap().0, 1);
                for i in 0..256 {
                    assert_eq!(
                        i64::decode(&mut input).unwrap(),
                        if i < 16 { 0x1111_1111_1111_1111 } else { 0 }
                    );
                }
            } else {
                assert_eq!(bits, 0);
                assert_eq!(VarInt::decode(&mut input).unwrap().0, 0);
            }
            assert_eq!(u8::decode(&mut input).unwrap(), 0);
            assert_eq!(VarInt::decode(&mut input).unwrap().0, 5);
        }
        assert!(input.is_empty());
        for h in chunk.heightmaps {
            for i in 0..256 {
                assert_eq!((h.data[i / 7] >> ((i % 7) * 9)) & 511, 129);
            }
        }
    }
}
#[cfg(test)]
mod keepalive_tests {
    use super::*;
    use ferrite_protocol::packets::configuration::{ClientboundKeepAlive, ServerboundKnownPacks};
    #[tokio::test]
    async fn configuration_wait_sends_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let task = tokio::spawn(async move {
            let mut wire = Wire::new(server);
            let mut decoder = FrameDecoder::default();
            let encoder = FrameEncoder::default();
            let mut timer = tokio::time::interval(std::time::Duration::from_millis(20));
            read_config_packet(&mut wire, &mut decoder, &encoder, &mut timer, None, None)
                .await
                .unwrap()
        });
        let mut decoder = FrameDecoder::default();
        let packet = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_packet(&mut client, &mut decoder),
        )
        .await
        .unwrap()
        .unwrap();
        let mut body = packet.as_slice();
        assert_eq!(
            VarInt::decode(&mut body).unwrap().0,
            ClientboundKeepAlive::ID
        );
        assert_eq!(ClientboundKeepAlive::decode(&mut body).unwrap().id, 1);
        send_packet(&mut client, &ServerboundKnownPacks { packs: vec![] })
            .await
            .unwrap();
        let packet = task.await.unwrap();
        assert_eq!(
            VarInt::decode(&mut packet.as_slice()).unwrap().0,
            ServerboundKnownPacks::ID
        );
    }
}
#[cfg(test)]
mod validation_tests {
    use super::*;
    #[tokio::test]
    async fn rejects_unrepresentable_compression_threshold() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            serve(
                listener,
                ServerConfig {
                    online_mode: false,
                    compression_threshold: i32::MAX as usize + 1,
                    ..Default::default()
                },
            ),
        )
        .await;
        assert!(result.is_ok_and(|r| r.is_err()));
    }
}
#[cfg(test)]
mod game_channel_tests {
    use super::game::{ServerboundPlay, decode_play};
    use ferrite_protocol::{encode_packet, packets::play::MovePosition};
    #[test]
    fn play_input_decodes_to_typed_event() {
        let mut bytes = Vec::new();
        encode_packet(
            &MovePosition {
                x: 5.0,
                y: 65.0,
                z: 0.5,
                flags: 1,
            },
            &mut bytes,
        );
        assert!(
            matches!(decode_play(&bytes).unwrap(),Some(ServerboundPlay::Position(p)) if p.x==5.0)
        );
    }
}
pub mod game;
