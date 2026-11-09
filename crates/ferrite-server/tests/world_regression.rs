//! Cross-system world regression through real bot sessions: a block change notifies its
//! neighbours, both tracking players receive the result, a second world reloads saved chunks,
//! and block changes keep working after players move into different region sections.
//!
//! `game::integration` is a `#[cfg(test)]`-private module, so this external test carries the
//! login/configuration/tracking client it needs instead of importing those helpers. The
//! server-side APIs under test are public (`World`, `AnvilChunkSource`, the protocol packets).

use ferrite_net::{ServerConfig, serve};
use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientboundKnownPacks, ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{
            BlockUpdate, ChunkBatchReceived, ChunkData, ClientKeepAlive, ConfirmTeleport,
            PlayKeepAlive, PositionSync, ServerboundChatCommand,
        },
    },
    types::BlockPos,
};
use ferrite_server::game::{AnvilChunkSource, Operator, World, WorldSettings};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const BLOCK_UPDATE: i32 = 8;
const CHUNK_DATA: i32 = 46;
const POSITION_SYNC: i32 = 73;
const CHUNK_BATCH_RECEIVED: i32 = 11;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives in <repo>/crates/<name>")
        .to_owned()
}

fn load_registry() -> Option<ferrite_world::RuntimeRegistry> {
    ferrite_world::RuntimeRegistry::load(repo_root().join("vanilla/26.3/generated")).ok()
}

fn state(name: &str) -> u32 {
    u32::from(ferrite_data::blocks::default_state(name).unwrap())
}

fn air() -> u32 {
    state("air")
}

/// A minimal play-session client: login, configuration, position confirmation, chunk-batch
/// acknowledgements, block updates and commands.
struct Bot {
    stream: TcpStream,
    encoder: FrameEncoder,
    decoder: FrameDecoder,
}

impl Bot {
    async fn join(addr: SocketAddr, name: &str) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        let mut bot = Self {
            stream,
            encoder: FrameEncoder::default(),
            decoder: FrameDecoder::default(),
        };
        bot.send(&Intention {
            protocol_version: 777,
            server_address: "localhost".into(),
            server_port: addr.port(),
            intent: Intent::Login,
        })
        .await;
        bot.send(&LoginStart {
            name: name.into(),
            uuid: ferrite_net::offline_uuid(name),
        })
        .await;
        let first = bot.read().await;
        assert_eq!(VarInt::decode(&mut first.as_slice()).unwrap().0, 3);
        bot.encoder.set_compression(Some(256), 6);
        bot.decoder.set_compression(Some(256));
        let login = bot.read().await;
        assert_eq!(VarInt::decode(&mut login.as_slice()).unwrap().0, 2);
        bot.send(&LoginAcknowledged).await;
        loop {
            let frame = bot.read().await;
            let mut body = frame.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            if id == ClientboundKnownPacks::ID {
                let packs = ClientboundKnownPacks::decode(&mut body).unwrap();
                bot.send(&ServerboundKnownPacks { packs: packs.packs })
                    .await;
            }
            if id == 3 {
                break;
            }
        }
        bot.send(&AcknowledgeFinishConfiguration).await;
        bot
    }

    async fn send<P: Packet>(&mut self, packet: &P) {
        let mut body = Vec::new();
        encode_packet(packet, &mut body);
        let mut wire = Vec::new();
        self.encoder.encode(&body, &mut wire);
        self.stream.write_all(&wire).await.unwrap();
    }

    async fn read(&mut self) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(frame) = self.decoder.next_frame().unwrap() {
                    return frame;
                }
                let mut bytes = [0u8; 8192];
                let n = self.stream.read(&mut bytes).await.unwrap();
                assert!(n > 0, "server closed the connection");
                self.decoder.feed(&bytes[..n]);
            }
        })
        .await
        .expect("timed out waiting for a packet")
    }

    async fn next_play(&mut self) -> (i32, Vec<u8>) {
        let bytes = self.read().await;
        let mut body = bytes.as_slice();
        let id = VarInt::decode(&mut body).unwrap().0;
        let rest = body.to_vec();
        if id == ferrite_protocol::packets::play::PlayDisconnect::ID {
            let reason =
                ferrite_protocol::packets::play::PlayDisconnect::decode(&mut rest.as_slice())
                    .unwrap();
            panic!("server disconnected the bot: {}", reason.reason.0);
        }
        match id {
            POSITION_SYNC => {
                let mut body = rest.as_slice();
                let sync = PositionSync::decode(&mut body).unwrap();
                self.send(&ConfirmTeleport {
                    id: sync.teleport_id,
                    x: sync.x,
                    y: sync.y,
                    z: sync.z,
                    yaw: sync.yaw,
                    pitch: sync.pitch,
                })
                .await;
            }
            CHUNK_BATCH_RECEIVED => {
                self.send(&ChunkBatchReceived {
                    chunks_per_tick: 64.0,
                })
                .await;
            }
            45 => {
                let mut body = rest.as_slice();
                let keepalive = PlayKeepAlive::decode(&mut body).unwrap();
                self.send(&ClientKeepAlive { id: keepalive.id }).await;
            }
            _ => {}
        }
        (id, rest)
    }

    /// Waits until the server has sent the given chunk, which also means the player tracks it.
    async fn wait_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (id, body) = self.next_play().await;
                if id == CHUNK_DATA {
                    let mut body = body.as_slice();
                    let data = ChunkData::decode(&mut body).unwrap();
                    if data.x == chunk_x && data.z == chunk_z {
                        return;
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("chunk {chunk_x},{chunk_z} was never sent"));
    }

    async fn command(&mut self, command: &str) {
        self.send(&ServerboundChatCommand {
            command: command.into(),
        })
        .await;
    }

    /// Reads updates until the block at `at` is broadcast, returning its new state.
    async fn wait_block(&mut self, at: (i32, i32, i32)) -> u32 {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let (id, body) = self.next_play().await;
                if id != BLOCK_UPDATE {
                    continue;
                }
                let update = BlockUpdate::decode(&mut body.as_slice()).unwrap();
                let pos = BlockPos::unpack(update.location);
                if (pos.x, pos.y, pos.z) == at {
                    return u32::try_from(update.block_state.0).unwrap_or(0);
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no block update at {at:?}"))
    }

    /// Asserts that no block update for `at` arrives within a short window.
    async fn expect_no_block(&mut self, at: (i32, i32, i32)) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(750);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return;
            }
            let Ok((id, body)) = tokio::time::timeout(remaining, self.next_play()).await else {
                return;
            };
            if id != BLOCK_UPDATE {
                continue;
            }
            let update = BlockUpdate::decode(&mut body.as_slice()).unwrap();
            let pos = BlockPos::unpack(update.location);
            assert_ne!(
                (pos.x, pos.y, pos.z),
                at,
                "a player outside the chunk's region received its block update"
            );
        }
    }
}

/// Starts a server for `world` on an ephemeral port.
async fn serve_world(world: &World) -> (SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(
        listener,
        ServerConfig {
            online_mode: false,
            view_distance: 2,
            // A bot sends nothing between commands; the test world can take longer than the
            // default read timeout to tick and flush, so keep the session alive explicitly.
            read_timeout: Duration::from_secs(120),
            // Two bot sessions read slowly between assertions; a burst of chunk packets must
            // not latch the slow-reader overflow path while the server is under test.
            send_queue_bytes: 16 * 1024 * 1024,
            game: Some(world.sender.clone()),
            ..Default::default()
        },
    ));
    (addr, server)
}

fn operator(name: &str) -> Operator {
    Operator {
        uuid: ferrite_net::offline_uuid(name),
        level: 4,
    }
}

/// Both players see a command's block change and the neighbour update that removes a torch
/// when its support disappears. After a save and a fresh world over the same directory, the
/// torched block is reloaded: removing its support still removes the saved torch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_players_share_neighbor_updates_and_reload_after_restart() {
    let Some(registry) = load_registry() else {
        eprintln!("skipping world regression test: vanilla/26.3/generated is absent");
        return;
    };
    let temp =
        std::env::temp_dir().join(format!("ferrite-world-regression-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).unwrap();

    let settings = || WorldSettings {
        ops: vec![operator("Alice"), operator("Bob")],
        ..Default::default()
    };
    let source = AnvilChunkSource::new(temp.clone(), registry);
    let world = World::start_configured(Some(source.clone()), [0, 65, 0], settings());
    let (addr, server) = serve_world(&world).await;

    let mut alice = Bot::join(addr, "Alice").await;
    let mut bob = Bot::join(addr, "Bob").await;
    alice.wait_chunk(0, 0).await;
    bob.wait_chunk(0, 0).await;

    alice.command("fill 0 65 0 2 65 2 minecraft:stone").await;
    assert_eq!(alice.wait_block((0, 65, 0)).await, state("stone"));
    assert_eq!(bob.wait_block((0, 65, 0)).await, state("stone"));

    alice.command("setblock 0 66 0 minecraft:torch").await;
    assert_eq!(alice.wait_block((0, 66, 0)).await, state("torch"));
    assert_eq!(bob.wait_block((0, 66, 0)).await, state("torch"));

    // Removing the support runs the neighbour shape update in both sessions.
    alice.command("setblock 0 65 0 minecraft:air").await;
    assert_eq!(alice.wait_block((0, 66, 0)).await, air());
    assert_eq!(bob.wait_block((0, 66, 0)).await, air());

    // Leave a supported torch that must survive the save/restart.
    alice.command("setblock 4 65 0 minecraft:stone").await;
    assert_eq!(alice.wait_block((4, 65, 0)).await, state("stone"));
    alice.command("setblock 4 66 0 minecraft:torch").await;
    assert_eq!(alice.wait_block((4, 66, 0)).await, state("torch"));
    assert_eq!(bob.wait_block((4, 66, 0)).await, state("torch"));

    drop(alice);
    drop(bob);
    server.abort();
    world.shutdown().await.unwrap();

    let Some(registry) = load_registry() else {
        panic!("vanilla data disappeared during the test");
    };
    let reloaded = AnvilChunkSource::new(temp.clone(), registry);
    let world = World::start_configured(Some(reloaded.clone()), [0, 65, 0], settings());
    let (addr, server) = serve_world(&world).await;
    let mut alice = Bot::join(addr, "Alice").await;
    let mut bob = Bot::join(addr, "Bob").await;
    alice.wait_chunk(0, 0).await;
    bob.wait_chunk(0, 0).await;

    // The chunk was read back from disk, not regenerated.
    assert_eq!(reloaded.block(4, 66, 0), Some(state("torch")));
    alice.command("setblock 4 65 0 minecraft:air").await;
    assert_eq!(alice.wait_block((4, 66, 0)).await, air());
    assert_eq!(bob.wait_block((4, 66, 0)).await, air());

    drop(alice);
    drop(bob);
    server.abort();
    world.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(temp);
}

/// Players in different region sections (256-block sections) only receive their own chunks'
/// updates; after one moves back beside the other, both receive the neighbour update again.
/// This exercises the chunk tracking hand-off around region split/rejoin without reaching
/// into private region state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_updates_follow_players_across_region_sections() {
    let world = World::start_configured(
        Some(AnvilChunkSource::new_memory()),
        [0, 65, 0],
        WorldSettings {
            ops: vec![operator("Alice"), operator("Bob")],
            ..Default::default()
        },
    );
    let (addr, server) = serve_world(&world).await;
    let mut alice = Bot::join(addr, "Alice").await;
    let mut bob = Bot::join(addr, "Bob").await;
    alice.wait_chunk(0, 0).await;
    bob.wait_chunk(0, 0).await;

    alice.command("fill 0 65 0 2 65 2 minecraft:stone").await;
    assert_eq!(alice.wait_block((0, 65, 0)).await, state("stone"));
    assert_eq!(bob.wait_block((0, 65, 0)).await, state("stone"));

    // Chunk 37 is in the next region section; Bob's chunks leave Alice's region.
    alice.command("tp Bob 600 65 0").await;
    bob.wait_chunk(37, 0).await;
    alice.command("setblock 0 66 0 minecraft:torch").await;
    assert_eq!(alice.wait_block((0, 66, 0)).await, state("torch"));
    bob.expect_no_block((0, 66, 0)).await;

    // Bob returns; the empty section is dropped and his tracking rejoins Alice's region.
    alice.command("tp Bob 2 65 2").await;
    bob.wait_chunk(0, 0).await;
    alice.command("setblock 0 65 0 minecraft:air").await;
    assert_eq!(alice.wait_block((0, 66, 0)).await, air());
    assert_eq!(bob.wait_block((0, 66, 0)).await, air());

    drop(alice);
    drop(bob);
    server.abort();
    world.shutdown().await.unwrap();
}
