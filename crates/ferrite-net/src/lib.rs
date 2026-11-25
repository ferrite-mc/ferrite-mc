pub mod cfb8;
pub mod forwarding;
pub mod rate_limit;
mod raw;
pub mod settings;
mod wire;
use anyhow::{Context, Result, bail, ensure};
use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        handshake::{Intent, Intention},
        status::{PingRequest, PongResponse, StatusRequest, StatusResponse},
    },
};
use raw::RawPacket;
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
/// How often the play writer sends a keepalive.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// Vanilla disconnects a play client that leaves a keepalive unanswered for 30 seconds.
pub const DEFAULT_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(30);

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
        response["favicon"] = serde_json::Value::String(favicon.to_string());
    }
    response.to_string()
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
    /// Disable packet compression completely (equivalent to `compression-threshold = -1`).
    pub disable_compression: bool,
    /// Zlib compression level (`1..=12` for libdeflate, clamped to backend range).
    pub compression_level: u32,
    /// Fall back to uncompressed wire frames when zlib increases total frame size.
    pub skip_incompressible: bool,
    /// Offload outbound compression + encryption to blocking pool when batch >= N bytes.
    pub writer_offload_bytes: usize,
    /// Maximum window to coalesce small outbound packets before flushing a TCP batch.
    pub batch_linger: Duration,
    pub view_distance: u8,
    /// Maximum queued, still-uncompressed packet bytes per connection. Exceeding it
    /// disconnects the client as timed out instead of blocking or growing without bound.
    pub send_queue_bytes: usize,
    /// Optional kernel send-buffer size for accepted connections. `None` keeps the operating
    /// system's default and autotuning; a small value makes a client that stops reading block
    /// the writer after a known number of bytes.
    pub socket_send_buffer_bytes: Option<usize>,
    /// How long one writer batch may block before the connection is dropped.
    pub write_timeout: std::time::Duration,
    /// Drop a play connection whose oldest keepalive has gone unanswered for this long.
    pub keepalive_timeout: Duration,
    pub forwarding: forwarding::Forwarding,
    /// `data:image/png;base64,...` shown in the server list, when a valid icon was loaded.
    /// Shared so the per-connection config clone does not copy the icon bytes.
    pub favicon: Option<Arc<str>>,
    /// Live players reported by the server-list ping.
    pub status: Arc<StatusState>,
    /// Handshake/status/login clients that send nothing for this long are dropped, like vanilla's
    /// Netty `ReadTimeoutHandler`.
    pub read_timeout: Duration,
    /// Chat and command token buckets (vanilla's spam threshold by default).
    pub rate_limits: rate_limit::RateLimits,
    /// Maximum concurrent accepted TCP connections before shedding excess sockets (`None` = unlimited).
    pub max_connections: Option<usize>,
    /// Per-source-IP TCP connection token bucket (`None` = unlimited).
    pub connection_rate_limit: Option<rate_limit::BucketConfig>,
    /// Apply `connection_rate_limit` to loopback addresses too (disabled by default for local load/tests).
    pub limit_loopback_connections: bool,
    /// Paper `unsupported-settings.perform-username-validation`. When true (the default) a login
    /// name must be 1-16 ASCII letters, digits or underscores. When false only the protocol's
    /// own limit (1-16 characters) applies, plus a floor that rejects control characters, which
    /// would otherwise reach logs, chat and commands unescaped.
    pub perform_username_validation: bool,
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
            disable_compression: false,
            compression_level: 6,
            skip_incompressible: true,
            writer_offload_bytes: WRITER_OFFLOAD_THRESHOLD_BYTES,
            batch_linger: Duration::from_millis(2),
            view_distance: 10,
            send_queue_bytes: 2 * 1024 * 1024,
            socket_send_buffer_bytes: None,
            write_timeout: std::time::Duration::from_secs(30),
            keepalive_timeout: DEFAULT_KEEPALIVE_TIMEOUT,
            forwarding: forwarding::Forwarding::None,
            favicon: None,
            status: Arc::new(StatusState::default()),
            read_timeout: DEFAULT_READ_TIMEOUT,
            rate_limits: rate_limit::RateLimits::default(),
            max_connections: Some(4096),
            connection_rate_limit: Some(rate_limit::BucketConfig {
                burst: 20,
                per_second: 5.0,
            }),
            limit_loopback_connections: false,
            perform_username_validation: true,
            game: None,
        }
    }
}

pub async fn serve(listener: TcpListener, config: ServerConfig) -> Result<()> {
    tokio::task::spawn_blocking(|| {
        let _ = registry::cached_vanilla();
    });
    if !config.disable_compression {
        i32::try_from(config.compression_threshold)
            .context("compression threshold exceeds protocol limit")?;
    }
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
    let conn_slots = config
        .max_connections
        .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit)));
    let mut ip_limiter = config.connection_rate_limit.map(|limit| {
        rate_limit::IpConnectionLimiter::new(limit, config.limit_loopback_connections)
    });
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                // A transient accept failure (EMFILE under a connection burst, for example)
                // must not tear down the listener for every already connected player.
                tracing::warn!(%error, "accept failed; continuing");
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
        };
        if let Some(limiter) = &mut ip_limiter
            && !limiter.check(peer.ip(), std::time::Instant::now())
        {
            tracing::warn!(%peer, "dropping TCP connection: IP connection rate limit exceeded");
            continue;
        }
        let permit = match &conn_slots {
            Some(sem) => match Arc::clone(sem).try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    tracing::warn!(%peer, "dropping TCP connection: max_connections limit reached");
                    continue;
                }
            },
            None => None,
        };
        stream.set_nodelay(true)?;
        if let Some(bytes) = config.socket_send_buffer_bytes {
            socket2::SockRef::from(&stream)
                .set_send_buffer_size(bytes)
                .context("setting the accepted socket's send buffer")?;
        }
        let key = key.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = handle_connection(Wire::new(stream), config, key, peer.ip()).await {
                tracing::info!(%peer, %error, "connection closed");
            }
        });
    }
}

async fn read_play_packet<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    decoder: &mut FrameDecoder,
) -> Result<(bool, Option<game::ServerboundPlay>)> {
    loop {
        if let Some(frame) = decoder.next_frame_ref()? {
            let ka = settings::is_keepalive_answer(frame);
            let pkt = game::decode_play(frame)?;
            return Ok((ka, pkt));
        }
        let mut bytes = [0u8; 4096];
        let n = stream.read(&mut bytes).await?;
        if n == 0 {
            bail!("connection closed");
        }
        decoder.feed(&bytes[..n]);
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

/// Vanilla's `multiplayer.disconnect.outdated_client` / `outdated_server`, as the login-state
/// disconnect (ID 0, JSON text).
fn outdated_login_disconnect(client_protocol: i32) -> RawPacket {
    use ferrite_protocol::Encode;
    let key = if client_protocol < ferrite_protocol::PROTOCOL_VERSION {
        "multiplayer.disconnect.outdated_client"
    } else {
        "multiplayer.disconnect.outdated_server"
    };
    let reason = serde_json::json!({
        "translate": key,
        "with": [ferrite_protocol::GAME_VERSION],
    })
    .to_string();
    let mut body = Vec::new();
    reason.encode(&mut body);
    RawPacket::new(0, body)
}

/// Encode a packet and send it to the client.
async fn send_raw<W: tokio::io::AsyncWrite + Unpin>(
    stream: &mut W,
    encoder: &FrameEncoder,
    packet: RawPacket,
) -> Result<()> {
    let mut wire = Vec::new();
    encoder.encode(&packet.to_frame(), &mut wire);
    stream.write_all(&wire).await?;
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
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != Intention::ID {
        bail!("expected handshake");
    }
    let handshake = Intention::decode(&mut body)?;
    // A login from another protocol is rejected in the login state, like vanilla's handshake
    // listener. Status still answers so the server list shows the version mismatch.
    if handshake.intent != Intent::Status {
        match ferrite_protocol::negotiate_protocol(handshake.protocol_version) {
            ferrite_protocol::NegotiationOutcome::Supported(_) => {}
            ferrite_protocol::NegotiationOutcome::OutdatedClient {
                client_protocol, ..
            }
            | ferrite_protocol::NegotiationOutcome::OutdatedServer {
                client_protocol, ..
            } => {
                let disconnect_encoder = FrameEncoder::default();
                send_raw(
                    &mut stream,
                    &disconnect_encoder,
                    outdated_login_disconnect(client_protocol),
                )
                .await?;
                return Ok(());
            }
        }
    }
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
        return offline_login(
            stream,
            decoder,
            config,
            key,
            legacy,
            peer,
            (handshake.server_address.clone(), handshake.server_port),
        )
        .await;
    }
    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != StatusRequest::ID {
        bail!("expected status request");
    }
    StatusRequest::decode(&mut body)?;
    let json = status_json(&config);
    let status_encoder = FrameEncoder::default();
    send_raw(
        &mut stream,
        &status_encoder,
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
    virtual_host: (String, u16),
) -> Result<()> {
    use ferrite_protocol::packets::login::{LoginStart, LoginSuccess, SetCompression};

    let packet = read_packet_timeout(&mut stream, &mut decoder, config.read_timeout).await?;
    let mut body = packet.as_slice();
    if VarInt::decode(&mut body)?.0 != LoginStart::ID {
        bail!("expected login start");
    }
    let start = LoginStart::decode(&mut body)?;
    if !username_allowed(&start.name, config.perform_username_validation) {
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
    settings::record_virtual_host(profile.uuid, &virtual_host.0, virtual_host.1);
    let mut encoder = FrameEncoder::default();
    if !config.disable_compression {
        send_packet(
            &mut stream,
            &SetCompression {
                threshold: VarInt(config.compression_threshold as i32),
            },
        )
        .await?;
        decoder.set_compression(Some(config.compression_threshold));
        encoder.set_compression(Some(config.compression_threshold), config.compression_level);
        encoder.set_skip_incompressible(config.skip_incompressible);
    }
    send_raw(
        &mut stream,
        &encoder,
        RawPacket::of(&LoginSuccess {
            uuid: profile.uuid,
            name: profile.name.clone(),
            properties: profile.properties.clone(),
            session_id: uuid::Uuid::new_v4(),
        }),
    )
    .await?;
    configure(stream, decoder, encoder, config, profile).await
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
type ConfigWireKey = (Option<usize>, u32, bool);
static CONFIG_WIRE_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<ConfigWireKey, Arc<[u8]>>>,
> = std::sync::OnceLock::new();

fn config_registry_wire(encoder: &FrameEncoder, vanilla: &registry::VanillaData) -> Arc<[u8]> {
    let key = encoder.compression_config();
    let cache =
        CONFIG_WIRE_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(wire) = cache.lock().unwrap().get(&key).cloned() {
        return wire;
    }
    let mut wire = Vec::with_capacity(256 * 1024);
    for (name, entries) in &vanilla.registries {
        let frame = RawPacket::of(&ferrite_protocol::packets::configuration::RegistryData {
            registry: name.clone(),
            entries: entries.clone(),
        })
        .to_frame();
        encoder.encode(&frame, &mut wire);
    }
    encoder.encode(&RawPacket::of(&vanilla.tags).to_frame(), &mut wire);
    encoder.encode(
        &RawPacket::of(&ferrite_protocol::packets::configuration::FinishConfiguration).to_frame(),
        &mut wire,
    );
    let arc: Arc<[u8]> = Arc::from(wire);
    cache
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| Arc::clone(&arc));
    arc
}

async fn configure(
    mut stream: Wire,
    mut decoder: FrameDecoder,
    encoder: FrameEncoder,
    config: ServerConfig,
    mut profile: Profile,
) -> Result<()> {
    use ferrite_protocol::{
        Encode, RestBytes,
        packets::{
            configuration::{
                AcknowledgeFinishConfiguration, ClientboundCustomPayload, ClientboundKnownPacks,
                KnownPack, ServerboundKnownPacks,
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
    let pack = KnownPack {
        namespace: "minecraft".into(),
        id: "core".into(),
        version: ferrite_protocol::GAME_VERSION.into(),
    };
    let mut init_wire = Vec::new();
    encoder.encode(
        &RawPacket::of(&ClientboundCustomPayload {
            channel: "minecraft:brand".into(),
            data: RestBytes(brand),
        })
        .to_frame(),
        &mut init_wire,
    );
    encoder.encode(
        &RawPacket::of(&ClientboundKnownPacks {
            packs: vec![pack.clone()],
        })
        .to_frame(),
        &mut init_wire,
    );
    stream.write_all(&init_wire).await?;
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
            config.read_timeout,
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
    let vanilla = match registry::try_cached_vanilla() {
        Some(cached) => cached,
        None => tokio::task::spawn_blocking(registry::cached_vanilla).await??,
    };
    let config_wire = config_registry_wire(&encoder, &vanilla);
    stream.write_all(&config_wire).await?;
    loop {
        let packet = read_config_packet(
            &mut stream,
            &mut decoder,
            &encoder,
            &mut timer,
            Some(&mut profile),
            config.read_timeout,
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
    play(stream, decoder, encoder, config, profile, vanilla).await
}
async fn read_config_packet(
    stream: &mut Wire,
    decoder: &mut FrameDecoder,
    encoder: &FrameEncoder,
    timer: &mut tokio::time::Interval,
    mut profile: Option<&mut Profile>,
    read_timeout: Duration,
) -> Result<Vec<u8>> {
    use ferrite_protocol::packets::configuration::{
        ClientInformation, ClientboundKeepAlive, ServerboundCustomPayload, ServerboundKeepAlive,
    };
    loop {
        tokio::select! {
            _=timer.tick()=>{send_raw(stream,encoder,RawPacket::of(&ClientboundKeepAlive{id:1})).await?;}
            packet=tokio::time::timeout(read_timeout,read_packet(stream,decoder))=>{
                let frame=match packet{
                    Ok(frame)=>frame?,
                    Err(_)=>bail!("timed out after {read_timeout:?} waiting for a configuration packet"),
                };
                {
                    let packet = RawPacket::parse(&frame)?;
                    let mut body = packet.body.as_slice();
                    match packet.id {
                        0 => { let info = ClientInformation::decode(&mut body)?; if let Some(profile)=profile.as_deref_mut(){settings::record(profile.uuid,settings::ClientSettings{locale:info.locale.clone(),view_distance:info.view_distance,left_handed:info.main_hand.0==0,chat_mode:info.chat_mode.0,chat_colors:info.chat_colors,skin_parts:info.skin_parts,text_filtering:info.text_filtering,server_listing:info.server_listing,particle_status:info.particle_status.0});profile.chat_visibility=match info.chat_mode.0{1=>1,2=>2,_=>0};} },
                        2 => { let payload = ServerboundCustomPayload::decode(&mut body)?; if payload.channel == "minecraft:brand" && let Some(profile) = profile.as_deref() { settings::record_brand_payload(profile.uuid, &payload.data.0); } },
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
    vanilla: Arc<registry::VanillaData>,
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
    let probe = settings::PingProbe::new(uuid);
    let mut writer_task = tokio::spawn(write_loop_with(
        writer,
        probe.clone(),
        encoder,
        queue,
        WriterOptions {
            write_timeout: config.write_timeout,
            offload_bytes: config.writer_offload_bytes,
            batch_linger: config.batch_linger,
            keepalive_interval: KEEPALIVE_INTERVAL,
            keepalive_timeout: config.keepalive_timeout,
        },
    ));
    let mut limiter =
        rate_limit::PlayerLimiter::new(&config.rate_limits, std::time::Instant::now());
    // Set once the spam kick is queued: the writer delivers it and ends the connection, so stop
    // forwarding input meanwhile.
    let mut kicked = false;
    let result = async {
        loop {
            tokio::select! {
                finished = &mut writer_task => {
                    let res = match finished {
                        Ok(result) => result,
                        Err(error) => Err(anyhow::anyhow!("writer task failed: {error}")),
                    };
                    if res.is_ok() {
                        // Briefly drain any unread client bytes after `shutdown_write` (TCP FIN)
                        // so Windows Winsock does not emit TCP RST (WSAECONNABORTED 10053) when
                        // dropping the socket while the client is still reading `PlayDisconnect`.
                        let mut drain = [0u8; 4096];
                        let _ = tokio::time::timeout(Duration::from_millis(50), async {
                            while matches!(reader.read(&mut drain).await, Ok(n) if n > 0) {}
                        })
                        .await;
                    }
                    return res;
                }
                frame = read_play_packet(&mut reader, &mut decoder) => {
                    let (is_ka, decoded) = frame?;
                    if is_ka {
                        probe.answered();
                    }
                    if let Some(packet) = decoded {
                        if kicked {
                            continue;
                        }
                        if matches!(packet, game::ServerboundPlay::ClientTickEnd) {
                            continue;
                        }
                        if let game::ServerboundPlay::ClientInfo(ref info) = packet {
                            settings::record(
                                uuid,
                                settings::ClientSettings {
                                    locale: info.locale.clone(),
                                    view_distance: info.view_distance,
                                    left_handed: info.main_hand.0 == 0,
                                    chat_mode: info.chat_mode.0,
                                    chat_colors: info.chat_colors,
                                    skin_parts: info.skin_parts,
                                    text_filtering: info.text_filtering,
                                    server_listing: info.allow_listing,
                                    particle_status: info.particle_status.0,
                                },
                            );
                        }
                        match limiter.check(&packet, std::time::Instant::now()) {
                            rate_limit::Verdict::Allow => {}
                            rate_limit::Verdict::Drop => continue,
                            rate_limit::Verdict::Kick => {
                                kicked = true;
                                let _ = outbound.send(game::Outbound::Disconnect(
                                    rate_limit::SPAM_KICK_REASON.to_string(),
                                ));
                                continue;
                            }
                        }
                        game.send(GameMessage::Input { session, uuid, packet })
                            .map_err(|_| anyhow::anyhow!("game loop stopped"))?;
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
/// Batches at or above this raw byte size offload compression and CFB8 encryption to
/// `spawn_blocking` so large chunk bursts do not stall sibling tokio socket tasks.
const WRITER_OFFLOAD_THRESHOLD_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct WriterOptions {
    write_timeout: std::time::Duration,
    offload_bytes: usize,
    batch_linger: std::time::Duration,
    keepalive_interval: Duration,
    keepalive_timeout: Duration,
}

#[cfg_attr(not(test), allow(dead_code))]
async fn write_loop<W: wire::BatchWriter + Send + 'static>(
    writer: W,
    probe: std::sync::Arc<settings::PingProbe>,
    encoder: FrameEncoder,
    queue: game::OutboundReceiver,
    write_timeout: std::time::Duration,
) -> Result<()> {
    write_loop_with(
        writer,
        probe,
        encoder,
        queue,
        WriterOptions {
            write_timeout,
            offload_bytes: WRITER_OFFLOAD_THRESHOLD_BYTES,
            batch_linger: std::time::Duration::from_millis(2),
            keepalive_interval: KEEPALIVE_INTERVAL,
            keepalive_timeout: DEFAULT_KEEPALIVE_TIMEOUT,
        },
    )
    .await
}

async fn write_loop_with<W: wire::BatchWriter + Send + 'static>(
    mut writer: W,
    probe: std::sync::Arc<settings::PingProbe>,
    encoder: FrameEncoder,
    mut queue: game::OutboundReceiver,
    options: WriterOptions,
) -> Result<()> {
    let WriterOptions {
        write_timeout,
        offload_bytes,
        batch_linger,
        keepalive_interval,
        keepalive_timeout,
    } = options;
    use ferrite_protocol::{
        packets::play::{PlayDisconnect, PlayKeepAlive},
        types::PlainText,
    };
    let encoder = Arc::new(encoder);
    let keepalive_body: Arc<[u8]> = {
        let mut buf = Vec::new();
        encode_packet(&PlayKeepAlive { id: 1 }, &mut buf);
        buf.into()
    };
    let mut keepalive = tokio::time::interval_at(
        tokio::time::Instant::now() + keepalive_interval,
        keepalive_interval,
    );
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut batch_buf: Vec<u8> = Vec::with_capacity(8192);
    let mut bodies: Vec<QueuedBody> = Vec::with_capacity(64);
    loop {
        bodies.clear();
        let mut close = None;
        tokio::select! {
            biased;
            _ = keepalive.tick() => {
                // A client that never answers (or a half-open connection) must not hold a
                // player slot, its writer task and the outbound budget forever.
                if probe
                    .unanswered_for()
                    .is_some_and(|age| age >= keepalive_timeout)
                {
                    close = Some("Timed out".to_string());
                } else {
                    probe.sent();
                    bodies.push(QueuedBody::Raw(Arc::clone(&keepalive_body)));
                }
            }
            incoming = queue.next() => match incoming {
                game::Incoming::Overflow => {
                    queue.discard();
                    close = Some("Timed out".to_string());
                }
                game::Incoming::Closed => {
                    let _ = tokio::time::timeout(write_timeout, writer.shutdown_write()).await;
                    return Ok(());
                }
                game::Incoming::Message(msg) => {
                    queue_body(&mut bodies, msg, &mut close);
                    // Keep collecting briefly so packets pushed by regions leave in one batch.
                    if !batch_linger.is_zero() {
                        let linger = tokio::time::sleep(batch_linger);
                        tokio::pin!(linger);
                        while close.is_none() {
                            tokio::select! {
                                biased;
                                _ = &mut linger => break,
                                more = queue.next() => match more {
                                    game::Incoming::Message(msg) => {
                                        queue_body(&mut bodies, msg, &mut close);
                                    }
                                    game::Incoming::Overflow => {
                                        queue.discard();
                                        close = Some("Timed out".to_string());
                                    }
                                    game::Incoming::Closed => {
                                        if !bodies.is_empty() {
                                            flush_bodies_with(
                                                &mut writer,
                                                &encoder,
                                                &mut bodies,
                                                &mut batch_buf,
                                                write_timeout,
                                                offload_bytes,
                                            )
                                            .await?;
                                        }
                                        let _ = tokio::time::timeout(
                                            write_timeout,
                                            writer.shutdown_write(),
                                        )
                                        .await;
                                        return Ok(());
                                    }
                                },
                            }
                        }
                    }
                    while close.is_none()
                        && let Some(msg) = queue.try_recv()
                    {
                        queue_body(&mut bodies, msg, &mut close);
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
            tracing::info!(%reason, "disconnecting player");
            let mut body = Vec::new();
            encode_packet(
                &PlayDisconnect {
                    reason: PlainText(reason),
                },
                &mut body,
            );
            bodies.push(QueuedBody::Raw(body.into()));
            flush_bodies_with(
                &mut writer,
                &encoder,
                &mut bodies,
                &mut batch_buf,
                write_timeout,
                offload_bytes,
            )
            .await?;
            let _ = tokio::time::timeout(write_timeout, writer.shutdown_write()).await;
            return Ok(());
        }
        if !bodies.is_empty() {
            flush_bodies_with(
                &mut writer,
                &encoder,
                &mut bodies,
                &mut batch_buf,
                write_timeout,
                offload_bytes,
            )
            .await?;
        }
    }
}
enum QueuedBody {
    Raw(Arc<[u8]>),
    Shared(Arc<game::SharedFrame>),
}
impl QueuedBody {
    fn raw_len(&self) -> usize {
        match self {
            Self::Raw(body) => body.len(),
            Self::Shared(shared) => shared.raw.len(),
        }
    }
    fn wire_len(&self, enc_cfg: (Option<usize>, u32, bool)) -> usize {
        match self {
            Self::Raw(body) => body.len() + 10,
            Self::Shared(shared)
                if (shared.threshold, shared.level, shared.skip_incompressible) == enc_cfg =>
            {
                shared.framed.len()
            }
            Self::Shared(shared) => shared.raw.len() + 10,
        }
    }
    fn work_bytes(&self, enc_cfg: (Option<usize>, u32, bool), encrypted: bool) -> usize {
        match self {
            Self::Shared(shared)
                if (shared.threshold, shared.level, shared.skip_incompressible) == enc_cfg =>
            {
                if encrypted {
                    shared.framed.len()
                } else {
                    0
                }
            }
            _ => self.raw_len(),
        }
    }
    fn encode_into(
        &self,
        encoder: &FrameEncoder,
        enc_cfg: (Option<usize>, u32, bool),
        out: &mut Vec<u8>,
    ) {
        match self {
            Self::Raw(body) => encoder.encode(body, out),
            Self::Shared(shared)
                if (shared.threshold, shared.level, shared.skip_incompressible) == enc_cfg =>
            {
                out.extend_from_slice(&shared.framed);
            }
            Self::Shared(shared) => encoder.encode(&shared.raw, out),
        }
    }
}
fn queue_body(bodies: &mut Vec<QueuedBody>, msg: game::Outbound, close: &mut Option<String>) {
    match msg {
        // Zero-copy handle to uncompressed or pre-compressed packet payload.
        game::Outbound::Packet(body) => bodies.push(QueuedBody::Raw(body)),
        game::Outbound::Batch(packets) => {
            bodies.extend(packets.into_iter().map(QueuedBody::Raw));
        }
        game::Outbound::SharedFrame(shared) => bodies.push(QueuedBody::Shared(shared)),
        game::Outbound::Disconnect(reason) => *close = Some(reason),
    }
}
async fn flush_bodies_with<W: wire::BatchWriter>(
    writer: &mut W,
    encoder: &Arc<FrameEncoder>,
    bodies: &mut Vec<QueuedBody>,
    batch_buf: &mut Vec<u8>,
    write_timeout: std::time::Duration,
    offload_bytes: usize,
) -> Result<()> {
    let enc_cfg = encoder.compression_config();
    let encrypted = writer.has_cipher();
    // One pass for both figures: this runs for every writer batch on every connection.
    let mut work_bytes = 0usize;
    let mut out_cap = 0usize;
    for body in bodies.iter() {
        work_bytes += body.work_bytes(enc_cfg, encrypted);
        out_cap += body.wire_len(enc_cfg);
    }
    if work_bytes >= offload_bytes {
        let enc = Arc::clone(encoder);
        let mut cipher = writer.take_cipher();
        let mut offloaded_bodies = std::mem::take(bodies);
        let mut offloaded_buf = std::mem::take(batch_buf);
        let (cipher, returned_bodies, returned_buf) = tokio::task::spawn_blocking(move || {
            offloaded_buf.clear();
            if offloaded_buf.capacity() < out_cap {
                offloaded_buf.reserve(out_cap - offloaded_buf.capacity());
            }
            for body in &offloaded_bodies {
                body.encode_into(&enc, enc_cfg, &mut offloaded_buf);
            }
            if let Some(c) = &mut cipher {
                c.encrypt(&mut offloaded_buf);
            }
            offloaded_bodies.clear();
            (cipher, offloaded_bodies, offloaded_buf)
        })
        .await
        .context("writer offload task failed")?;
        *bodies = returned_bodies;
        *batch_buf = returned_buf;
        writer.restore_cipher(cipher);
        let res = match tokio::time::timeout(write_timeout, writer.write_raw_batch(batch_buf)).await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.into()),
            Err(_) => bail!("client timed out while writing"),
        };
        if batch_buf.capacity() > 512 * 1024 {
            batch_buf.clear();
            batch_buf.shrink_to(65_536);
        }
        res
    } else {
        batch_buf.clear();
        if batch_buf.capacity() < out_cap {
            batch_buf.reserve(out_cap - batch_buf.capacity());
        }
        for body in &*bodies {
            body.encode_into(encoder, enc_cfg, batch_buf);
        }
        bodies.clear();
        let res = write_batch(writer, batch_buf, write_timeout).await;
        if batch_buf.capacity() > 512 * 1024 {
            batch_buf.clear();
            batch_buf.shrink_to(65_536);
        }
        res
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
/// Shared, pooled client for Mojang session authentication.
///
/// Building a client per login would rebuild the TLS configuration and connection pool, and a
/// request without a timeout could hold the login task (and its connection slot) forever.
fn session_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
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
    let response = session_client()
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
/// The login-name check: strict by default, protocol-limit-only when validation is turned off.
fn username_allowed(name: &str, perform_validation: bool) -> bool {
    if perform_validation {
        return valid_username(name);
    }
    !name.is_empty() && name.chars().count() <= 16 && !name.chars().any(char::is_control)
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
            settings::PingProbe::new(uuid::Uuid::nil()),
            FrameEncoder::default(),
            rx,
            Duration::from_secs(5),
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
    async fn silent_configuration_client_is_disconnected_by_the_read_timeout() {
        use ferrite_protocol::packets::login::{LoginAcknowledged, LoginStart};

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
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        send_packet(
            &mut stream,
            &Intention {
                protocol_version: ferrite_protocol::PROTOCOL_VERSION,
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
        let mut decoder = FrameDecoder::default();
        // SetCompression is the last uncompressed packet; LoginSuccess uses it.
        read_packet(&mut stream, &mut decoder).await.unwrap();
        decoder.set_compression(Some(256));
        let mut encoder = FrameEncoder::default();
        encoder.set_compression(Some(256), 6);
        read_packet(&mut stream, &mut decoder).await.unwrap();
        send_compressed(&mut stream, &encoder, &LoginAcknowledged)
            .await
            .unwrap();
        // The server sends the brand and known packs, then waits for
        // ServerboundKnownPacks. Staying silent must hit the configuration read timeout.
        let mut sink = [0u8; 1024];
        let mut closed = false;
        for _ in 0..16 {
            match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut sink)).await {
                Ok(Ok(0)) | Ok(Err(_)) => {
                    closed = true;
                    break;
                }
                Ok(Ok(_)) => {}
                Err(_) => break,
            }
        }
        assert!(
            closed,
            "server did not close a stalled configuration client"
        );
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
    async fn other_protocols_are_rejected_at_login() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
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
mod username_tests {
    use super::*;
    use ferrite_protocol::packets::{
        handshake::{Intent, Intention},
        login::{LoginStart, SetCompression},
    };
    use tokio::net::TcpStream;

    #[test]
    fn strict_validation_is_the_default() {
        assert!(ServerConfig::default().perform_username_validation);
        for ok in ["Steve", "a", "Name_16_chars_ok", "x1"] {
            assert!(username_allowed(ok, true), "{ok}");
        }
        for bad in [
            "",
            "Bad Name",
            "né",
            "a.b",
            "-x",
            "seventeen_chars_xx",
            "tab\t",
        ] {
            assert!(!username_allowed(bad, true), "{bad:?}");
        }
    }

    #[test]
    fn disabling_validation_allows_special_characters_but_keeps_the_floor() {
        for ok in ["Bad Name", "né", "a.b", "-x", "名前", "sixteen_chars_xx"] {
            assert!(username_allowed(ok, false), "{ok}");
        }
        // The protocol limit and the control-character floor still apply.
        for bad in ["", "seventeen_chars_xxx", "tab\t", "new\nline", "nul\0"] {
            assert!(!username_allowed(bad, false), "{bad:?}");
        }
    }

    /// Whether the server answered a login for `name` (it sends `SetCompression`) or hung up.
    async fn login_answered(name: &str, validate: bool) -> bool {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                perform_username_validation: validate,
                ..Default::default()
            },
        ));
        let mut stream = TcpStream::connect(addr).await.unwrap();
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
                name: name.into(),
                uuid: uuid::Uuid::nil(),
            },
        )
        .await
        .unwrap();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_packet(&mut stream, &mut decoder),
        )
        .await
        .expect("server neither answered nor closed")
        {
            Ok(packet) => VarInt::decode(&mut packet.as_slice()).unwrap().0 == SetCompression::ID,
            Err(_) => false,
        }
    }

    #[tokio::test]
    async fn login_with_a_special_name_is_refused_unless_validation_is_off() {
        assert!(!login_answered("Bad Name", true).await);
        assert!(login_answered("Bad Name", false).await);
        // A normal name logs in either way.
        assert!(login_answered("Steve", true).await);
        assert!(login_answered("Steve", false).await);
    }
}
#[cfg(test)]
mod rate_limit_tests {
    use super::game::{GameMessage, ServerboundPlay};
    use super::rate_limit::{BucketConfig, LimitAction, RateLimits};
    use super::*;
    use ferrite_protocol::packets::{
        configuration::{AcknowledgeFinishConfiguration, FinishConfiguration},
        login::{LoginAcknowledged, LoginStart},
        play::{LastSeenUpdate, PlayDisconnect, ServerboundChat, ServerboundChatCommand},
    };
    use tokio::net::TcpStream;
    use tokio::sync::mpsc::UnboundedReceiver;

    struct Client {
        stream: TcpStream,
        encoder: FrameEncoder,
        decoder: FrameDecoder,
    }
    /// A server whose game loop only reports the chat and command packets it is handed.
    async fn start(limits: RateLimits) -> (Client, UnboundedReceiver<ServerboundPlay>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (game_tx, mut game_rx) = tokio::sync::mpsc::unbounded_channel();
        let (seen_tx, seen_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            // Hold the outbound sender so the writer stays open for the whole test.
            let mut _outbound = None;
            while let Some(message) = game_rx.recv().await {
                match message {
                    GameMessage::Joined { outbound, .. } => _outbound = Some(outbound),
                    GameMessage::Input { packet, .. } => {
                        let _ = seen_tx.send(packet);
                    }
                    _ => {}
                }
            }
        });
        tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                game: Some(game_tx),
                rate_limits: limits,
                ..Default::default()
            },
        ));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut encoder = FrameEncoder::default();
        let mut decoder = FrameDecoder::default();
        send_packet(
            &mut stream,
            &ferrite_protocol::packets::handshake::Intention {
                protocol_version: 777,
                server_address: "localhost".into(),
                server_port: addr.port(),
                intent: ferrite_protocol::packets::handshake::Intent::Login,
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
        loop {
            let bytes = read_packet(&mut stream, &mut decoder).await.unwrap();
            if VarInt::decode(&mut bytes.as_slice()).unwrap().0
                == ferrite_protocol::packets::configuration::ClientboundKnownPacks::ID
            {
                break;
            }
        }
        send_compressed(
            &mut stream,
            &encoder,
            &ferrite_protocol::packets::configuration::ServerboundKnownPacks {
                packs: vec![ferrite_protocol::packets::configuration::KnownPack {
                    namespace: "minecraft".into(),
                    id: "core".into(),
                    version: "26.3".into(),
                }],
            },
        )
        .await
        .unwrap();
        loop {
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                read_packet(&mut stream, &mut decoder),
            )
            .await
            .unwrap()
            .unwrap();
            if VarInt::decode(&mut bytes.as_slice()).unwrap().0 == FinishConfiguration::ID {
                break;
            }
        }
        send_compressed(&mut stream, &encoder, &AcknowledgeFinishConfiguration)
            .await
            .unwrap();
        (
            Client {
                stream,
                encoder,
                decoder,
            },
            seen_rx,
        )
    }
    async fn chat(client: &mut Client) {
        let packet = ServerboundChat {
            message: "hi".into(),
            timestamp: 0,
            salt: 0,
            signature: None,
            last_seen: LastSeenUpdate::new(0, &[], 0),
        };
        send_compressed(&mut client.stream, &client.encoder, &packet)
            .await
            .unwrap();
    }
    async fn command(client: &mut Client) {
        let packet = ServerboundChatCommand {
            command: "list".into(),
        };
        send_compressed(&mut client.stream, &client.encoder, &packet)
            .await
            .unwrap();
    }
    /// Collects what the game loop saw until it has been quiet for a moment.
    async fn drain(seen: &mut UnboundedReceiver<ServerboundPlay>) -> (usize, usize) {
        let (mut chats, mut commands) = (0, 0);
        while let Ok(Some(packet)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), seen.recv()).await
        {
            match packet {
                ServerboundPlay::Chat(_) => chats += 1,
                ServerboundPlay::ChatCommand(_) => commands += 1,
                _ => {}
            }
        }
        (chats, commands)
    }
    fn limits(chat: u32, command: u32, action: LimitAction) -> RateLimits {
        let bucket = |burst| {
            Some(BucketConfig {
                burst,
                per_second: 0.0,
            })
        };
        RateLimits {
            chat: bucket(chat),
            command: bucket(command),
            action,
        }
    }

    #[tokio::test]
    async fn spamming_chat_is_kicked_and_the_excess_never_reaches_the_game() {
        let (mut client, mut seen) = start(limits(2, 100, LimitAction::Kick)).await;
        for _ in 0..4 {
            chat(&mut client).await;
        }
        let mut reason = None;
        while reason.is_none() {
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                read_packet(&mut client.stream, &mut client.decoder),
            )
            .await
            .expect("no disconnect arrived")
            .expect("connection closed before the disconnect");
            let mut body = bytes.as_slice();
            if VarInt::decode(&mut body).unwrap().0 == PlayDisconnect::ID {
                reason = Some(PlayDisconnect::decode(&mut body).unwrap().reason.0);
            }
        }
        assert_eq!(reason.as_deref(), Some(rate_limit::SPAM_KICK_REASON));
        assert_eq!(drain(&mut seen).await, (2, 0));
    }

    #[tokio::test]
    async fn drop_action_discards_the_excess_and_keeps_the_connection() {
        let (mut client, mut seen) = start(limits(1, 100, LimitAction::Drop)).await;
        for _ in 0..3 {
            chat(&mut client).await;
        }
        // Commands have their own bucket and still pass after chat was throttled.
        command(&mut client).await;
        command(&mut client).await;
        assert_eq!(drain(&mut seen).await, (1, 2));
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
        let secret: [u8; 16] = std::array::from_fn(|_| rand::random());
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

    #[tokio::test]
    async fn encryption_handshake_rejects_bad_verify_token() {
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
        let key = RsaPublicKey::from_public_key_der(&request.public_key).unwrap();
        let secret: [u8; 16] = [0x11; 16];
        let mut wrong_token = request.verify_token.clone();
        wrong_token[0] ^= 0xFF;
        let mut rng = rand::thread_rng();
        send_packet(
            &mut stream,
            &EncryptionResponse {
                shared_secret: key.encrypt(&mut rng, Pkcs1v15Encrypt, &secret).unwrap(),
                verify_token: key
                    .encrypt(&mut rng, Pkcs1v15Encrypt, &wrong_token)
                    .unwrap(),
            },
        )
        .await
        .unwrap();
        // Server must close/fail the handshake without sending SetCompression or LoginSuccess.
        assert!(read_packet(&mut stream, &mut decoder).await.is_err());
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
    use ferrite_protocol::packets::{
        configuration::{ClientboundKeepAlive, ServerboundKnownPacks},
        play::PlayDisconnect,
    };
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
            read_config_packet(
                &mut wire,
                &mut decoder,
                &encoder,
                &mut timer,
                None,
                Duration::from_secs(5),
            )
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

    /// An unanswered keepalive must end the writer (and with it the connection), like vanilla's
    /// keepalive timeout. The short interval/timeout keep the test fast; the server uses 10 s/30 s.
    #[tokio::test]
    async fn unanswered_keepalive_disconnects_the_client() {
        struct RecordingWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl crate::wire::BatchWriter for RecordingWriter {
            async fn write_batch(&mut self, batch: &mut [u8]) -> std::io::Result<()> {
                self.0.lock().unwrap().extend_from_slice(batch);
                Ok(())
            }
        }

        let wire = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx, rx) = game::OutboundSender::channel(1024);
        let task = tokio::spawn(write_loop_with(
            RecordingWriter(std::sync::Arc::clone(&wire)),
            settings::PingProbe::new(uuid::Uuid::nil()),
            FrameEncoder::default(),
            rx,
            WriterOptions {
                write_timeout: Duration::from_secs(5),
                offload_bytes: usize::MAX, // keep the keepalives inline
                batch_linger: Duration::ZERO,
                keepalive_interval: Duration::from_millis(10),
                keepalive_timeout: Duration::from_millis(25),
            },
        ));
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the writer must enforce the keepalive timeout")
            .unwrap();
        assert!(result.is_ok());
        drop(tx);

        let bytes = wire.lock().unwrap().clone();
        let mut decoder = FrameDecoder::default();
        decoder.feed(&bytes);
        let frames: Vec<Vec<u8>> = std::iter::from_fn(|| decoder.next_frame().unwrap()).collect();
        assert!(
            frames.len() >= 2,
            "expected at least one keepalive before the disconnect"
        );
        let mut last = frames.last().unwrap().as_slice();
        assert_eq!(VarInt::decode(&mut last).unwrap().0, PlayDisconnect::ID);
        assert_eq!(
            PlayDisconnect::decode(&mut last).unwrap().reason.0,
            "Timed out"
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

    #[tokio::test]
    async fn offloaded_encrypted_writer_preserves_ordering_and_graceful_disconnect() {
        struct RecordingWriter {
            cipher: Option<crate::cfb8::Cfb8>,
            wire: Arc<std::sync::Mutex<Vec<u8>>>,
        }
        impl crate::wire::BatchWriter for RecordingWriter {
            async fn write_batch(&mut self, batch: &mut [u8]) -> std::io::Result<()> {
                if let Some(c) = &mut self.cipher {
                    c.encrypt(batch);
                }
                self.wire.lock().unwrap().extend_from_slice(batch);
                Ok(())
            }
            async fn write_raw_batch(&mut self, batch: &[u8]) -> std::io::Result<()> {
                self.wire.lock().unwrap().extend_from_slice(batch);
                Ok(())
            }
            fn take_cipher(&mut self) -> Option<crate::cfb8::Cfb8> {
                self.cipher.take()
            }
            fn restore_cipher(&mut self, cipher: Option<crate::cfb8::Cfb8>) {
                self.cipher = cipher;
            }
        }

        let key = [0x42u8; 16];
        let wire = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = RecordingWriter {
            cipher: Some(crate::cfb8::Cfb8::new(&key)),
            wire: Arc::clone(&wire),
        };
        let mut encoder = FrameEncoder::default();
        encoder.set_compression(Some(256), 6);
        let (tx, rx) = game::OutboundSender::channel(1024 * 1024);

        let task = tokio::spawn(write_loop(
            writer,
            settings::PingProbe::new(uuid::Uuid::nil()),
            encoder,
            rx,
            Duration::from_secs(5),
        ));

        // 1) Small inline packet (<4096)
        tx.send(game::Outbound::Packet(Arc::from(vec![1, 2, 3])))
            .unwrap();
        // 2) A per-viewer batch keeps its packets next to each other and in order.
        tx.send(game::Outbound::Batch(vec![
            Arc::from(vec![6u8]),
            Arc::from(vec![7u8, 8]),
        ]))
        .unwrap();
        // 3) Large offloaded packet (>=4096)
        let big = vec![0xABu8; 6000];
        tx.send(game::Outbound::Packet(Arc::from(big.clone())))
            .unwrap();
        // 4) Trailing small packet + graceful disconnect
        tx.send(game::Outbound::Packet(Arc::from(vec![4, 5])))
            .unwrap();
        tx.send(game::Outbound::Disconnect("bye".to_string()))
            .unwrap();

        task.await.unwrap().expect("write_loop exits cleanly");

        let mut ciphertext = wire.lock().unwrap().clone();
        let mut decrypt = crate::cfb8::Cfb8::new(&key);
        decrypt.decrypt(&mut ciphertext);
        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(256));
        dec.feed(&ciphertext);
        assert_eq!(dec.next_frame().unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(dec.next_frame().unwrap(), Some(vec![6]));
        assert_eq!(dec.next_frame().unwrap(), Some(vec![7, 8]));
        assert_eq!(dec.next_frame().unwrap(), Some(big));
        assert_eq!(dec.next_frame().unwrap(), Some(vec![4, 5]));
        // Last frame is PlayDisconnect("bye")
        let disc = dec.next_frame().unwrap().expect("disconnect frame");
        assert!(disc.windows(3).any(|w| w == b"bye"));
    }

    #[tokio::test]
    async fn writer_propagates_io_error_on_offloaded_batch() {
        struct FailingWriter;
        impl crate::wire::BatchWriter for FailingWriter {
            async fn write_batch(&mut self, _batch: &mut [u8]) -> std::io::Result<()> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
            }
            async fn write_raw_batch(&mut self, _batch: &[u8]) -> std::io::Result<()> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
            }
        }
        let mut encoder = FrameEncoder::default();
        encoder.set_compression(Some(256), 6);
        let (tx, rx) = game::OutboundSender::channel(1024 * 1024);
        let task = tokio::spawn(write_loop(
            FailingWriter,
            settings::PingProbe::new(uuid::Uuid::nil()),
            encoder,
            rx,
            Duration::from_secs(5),
        ));
        tx.send(game::Outbound::Packet(Arc::from(vec![0xCDu8; 5000])))
            .unwrap();
        let err = task.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[tokio::test]
    async fn custom_compression_level_and_offload_knobs_encode_cleanly() {
        let mut encoder = FrameEncoder::default();
        encoder.set_compression(Some(64), 12);
        encoder.set_skip_incompressible(true);

        let compressible = vec![b'A'; 2048];
        let mut compressed_wire = Vec::new();
        encoder.encode(&compressible, &mut compressed_wire);
        assert!(
            compressed_wire.len() < 256,
            "expected strong level-12 zlib compression"
        );

        let mut dec = FrameDecoder::default();
        dec.set_compression(Some(64));
        dec.feed(&compressed_wire);
        assert_eq!(dec.next_frame().unwrap(), Some(compressible));
    }

    #[tokio::test]
    async fn shared_frame_reuses_precompressed_wire_bytes_and_falls_back_on_mismatch() {
        struct RecordingWriter {
            wire: Arc<std::sync::Mutex<Vec<u8>>>,
        }
        impl crate::wire::BatchWriter for RecordingWriter {
            async fn write_batch(&mut self, batch: &mut [u8]) -> std::io::Result<()> {
                self.wire.lock().unwrap().extend_from_slice(batch);
                Ok(())
            }
        }
        let match_enc = FrameEncoder::new_compressed(Some(64), 6, true);
        let payload: Arc<[u8]> = vec![b'Z'; 512].into();
        let shared = game::shared_frame(&match_enc, payload.clone());

        // Matching encoder writes exact pre-framed bytes (with zero extra compression).
        let wire_match = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx1, rx1) = game::OutboundSender::channel(1024 * 1024);
        let t1 = tokio::spawn(write_loop(
            RecordingWriter {
                wire: Arc::clone(&wire_match),
            },
            settings::PingProbe::new(uuid::Uuid::nil()),
            match_enc,
            rx1,
            Duration::from_secs(5),
        ));
        tx1.send(shared.clone()).unwrap();
        drop(tx1);
        t1.await.unwrap().unwrap();

        let mut dec1 = FrameDecoder::default();
        dec1.set_compression(Some(64));
        dec1.feed(&wire_match.lock().unwrap());
        assert_eq!(dec1.next_frame().unwrap(), Some(payload.to_vec()));

        // Mismatched encoder threshold falls back to re-encoding `raw`.
        let diff_enc = FrameEncoder::new_compressed(Some(1024), 6, true);
        let wire_diff = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx2, rx2) = game::OutboundSender::channel(1024 * 1024);
        let t2 = tokio::spawn(write_loop(
            RecordingWriter {
                wire: Arc::clone(&wire_diff),
            },
            settings::PingProbe::new(uuid::Uuid::nil()),
            diff_enc,
            rx2,
            Duration::from_secs(5),
        ));
        tx2.send(shared).unwrap();
        drop(tx2);
        t2.await.unwrap().unwrap();

        let mut dec2 = FrameDecoder::default();
        dec2.set_compression(Some(1024));
        dec2.feed(&wire_diff.lock().unwrap());
        assert_eq!(dec2.next_frame().unwrap(), Some(payload.to_vec()));
        assert_ne!(&*wire_match.lock().unwrap(), &*wire_diff.lock().unwrap());
    }

    #[tokio::test]
    async fn serve_enforces_max_connections_and_ip_rate_limit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 1) max_connections = Some(1) drops second concurrent TCP socket cleanly.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                max_connections: Some(1),
                connection_rate_limit: None,
                read_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        ));

        let c1 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut c2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 16];
        let n2 = tokio::time::timeout(Duration::from_secs(2), c2.read(&mut buf))
            .await
            .expect("second connection should be dropped without stalling")
            .unwrap();
        assert_eq!(n2, 0);
        drop(c1);
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut c3 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut body = Vec::new();
        ferrite_protocol::encode_packet(
            &ferrite_protocol::packets::handshake::Intention {
                protocol_version: ferrite_protocol::PROTOCOL_VERSION,
                server_address: "127.0.0.1".into(),
                server_port: addr.port(),
                intent: ferrite_protocol::packets::handshake::Intent::Status,
            },
            &mut body,
        );
        let mut wire = Vec::new();
        FrameEncoder::default().encode(&body, &mut wire);
        body.clear();
        ferrite_protocol::encode_packet(
            &ferrite_protocol::packets::status::StatusRequest,
            &mut body,
        );
        FrameEncoder::default().encode(&body, &mut wire);
        c3.write_all(&wire).await.unwrap();
        let n3 = tokio::time::timeout(Duration::from_secs(2), c3.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert!(n3 > 0, "slot should be available after c1 closed");
        server.abort();

        // 2) IP rate limiter sheds burst flood when loopback limiting is enabled.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(
            listener,
            ServerConfig {
                online_mode: false,
                max_connections: None,
                connection_rate_limit: Some(rate_limit::BucketConfig {
                    burst: 1,
                    per_second: 0.0,
                }),
                limit_loopback_connections: true,
                read_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        ));

        let mut allowed = tokio::net::TcpStream::connect(addr).await.unwrap();
        allowed.write_all(&wire).await.unwrap();
        let n_allowed = tokio::time::timeout(Duration::from_secs(2), allowed.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert!(n_allowed > 0);

        let mut throttled = tokio::net::TcpStream::connect(addr).await.unwrap();
        let n_throttled = tokio::time::timeout(Duration::from_secs(2), throttled.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(n_throttled, 0);
        server.abort();
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
