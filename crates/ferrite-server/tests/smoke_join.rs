//! Headless smoke test: start the real `ferrite-server` binary, join it with a protocol bot,
//! check that the spawn position and chunks arrive, and stop it through the console.
//!
//! This covers what the in-process tests skip: argument and config parsing, the EULA gate,
//! binding the configured address, the full login/configuration/play bootstrap against the
//! shipped binary, and a clean exit. It uses the superflat generator, so it needs no vanilla
//! world generation host and no Java.

use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientInformation, ClientboundKnownPacks,
            ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{ChunkBatchReceived, ClientKeepAlive, ConfirmTeleport, PlayKeepAlive, PositionSync},
    },
};
use std::{
    io::Write,
    net::SocketAddr,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const CHUNK_DATA: i32 = 46;

struct Bot {
    stream: TcpStream,
    encoder: FrameEncoder,
    decoder: FrameDecoder,
}

impl Bot {
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

    /// Login, configuration and the first play packets, answering what a client must answer.
    async fn join(addr: SocketAddr, name: &str) -> Self {
        let mut bot = Self {
            stream: TcpStream::connect(addr).await.unwrap(),
            encoder: FrameEncoder::default(),
            decoder: FrameDecoder::default(),
        };
        bot.send(&Intention {
            protocol_version: ferrite_protocol::PROTOCOL_VERSION,
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
        let set_compression = bot.read().await;
        assert_eq!(
            VarInt::decode(&mut set_compression.as_slice()).unwrap().0,
            3
        );
        bot.encoder.set_compression(Some(256), 6);
        bot.decoder.set_compression(Some(256));
        let login_finished = bot.read().await;
        assert_eq!(VarInt::decode(&mut login_finished.as_slice()).unwrap().0, 2);
        bot.send(&LoginAcknowledged).await;
        bot.send(&ClientInformation {
            locale: "en_us".into(),
            view_distance: 2,
            chat_mode: VarInt(0),
            chat_colors: true,
            skin_parts: 0x7f,
            main_hand: VarInt(1),
            text_filtering: false,
            server_listing: true,
            particle_status: VarInt(0),
        })
        .await;
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

    /// Plays until the spawn position and at least `chunks` chunks have arrived.
    async fn until_spawned(&mut self, chunks: usize) {
        let (mut synced, mut seen) = (false, 0);
        while !(synced && seen >= chunks) {
            let frame = self.read().await;
            let mut body = frame.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            match id {
                73 => {
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
                    synced = true;
                }
                11 => {
                    self.send(&ChunkBatchReceived {
                        chunks_per_tick: 64.0,
                    })
                    .await;
                }
                45 => {
                    let keepalive = PlayKeepAlive::decode(&mut body).unwrap();
                    self.send(&ClientKeepAlive { id: keepalive.id }).await;
                }
                CHUNK_DATA => seen += 1,
                _ => {}
            }
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Keeps the child from outliving a failed assertion.
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_shipped_binary_starts_accepts_a_bot_and_stops_on_the_console() {
    let dir = std::env::temp_dir().join(format!("ferrite-smoke-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    std::fs::write(
        dir.join("ferrite.toml"),
        format!(
            "bind-address = '127.0.0.1:{port}'\nonline-mode = false\ngenerator = 'superflat'\nview-distance = 2\n"
        ),
    )
    .unwrap();
    // The server refuses to start without `eula=true`; this fixture runs only Ferrite.
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
            .arg("--headless")
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    // Startup loads registries and the world, so the port opens a little later.
    let deadline = Instant::now() + Duration::from_secs(120);
    let stream = loop {
        if let Ok(stream) = TcpStream::connect(addr).await {
            break stream;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "the server exited before it listened"
        );
        assert!(Instant::now() < deadline, "the server never listened");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    drop(stream);

    let mut bot = Bot::join(addr, "SmokeBot").await;
    // View distance 2 sends a 5x5 square; require the spawn column and its ring.
    tokio::time::timeout(Duration::from_secs(60), bot.until_spawned(9))
        .await
        .expect("the bot received a spawn position and chunks");
    drop(bot);

    writeln!(server.0.stdin.take().unwrap(), "/stop").unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the server did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "server exited with {status}");
    assert!(
        dir.join("world").exists(),
        "the world directory is created on startup"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
