//! In-game WorldEdit smoke test (gated on gitignored `compat/worldedit-bukkit-7.4.6-dist.jar`).
//!
//! Starts `ferrite-server --headless` with WorldEdit loaded, executes `/we version` on console,
//! connects a protocol bot, grants OP, runs `//pos1`, `//pos2`, and `//set minecraft:stone`,
//! and verifies command feedback and block updates.

use ferrite_protocol::{
    Decode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientInformation, ClientboundKnownPacks,
            ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{
            ChunkBatchReceived, ClientKeepAlive, ConfirmTeleport, PlayKeepAlive, PositionSync,
            ServerboundChatCommand, SystemChat,
        },
    },
};
use std::{
    io::{BufRead, BufReader, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
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
        tokio::time::timeout(Duration::from_secs(30), async {
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

    async fn until_spawned(&mut self, chunks: usize) -> (f64, f64, f64) {
        let mut synced = None;
        let mut seen = 0usize;
        while synced.is_none() || seen < chunks {
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
                    synced = Some((sync.x, sync.y, sync.z));
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
        synced.expect("spawn position")
    }

    async fn drain_chat_until(&mut self, needle: &str, timeout: Duration) -> Vec<String> {
        let mut messages = Vec::new();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Ok(frame) = tokio::time::timeout(remaining, self.read()).await else {
                break;
            };
            let mut body = frame.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            if id == SystemChat::ID {
                let chat = SystemChat::decode(&mut body).unwrap();
                let text = chat.content.0;
                let matched = text.contains(needle);
                messages.push(text);
                if matched {
                    break;
                }
            } else if id == 45 {
                let keepalive = PlayKeepAlive::decode(&mut body).unwrap();
                self.send(&ClientKeepAlive { id: keepalive.id }).await;
            }
        }
        messages
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn find_jar(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(prefix)
            && name.ends_with(".jar")
            && !name.contains("-sources")
            && !name.contains("-javadoc")
            && !name.contains("-paper")
        {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                best = Some((modified, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn spawn_log_collector<R: std::io::Read + Send + 'static>(
    reader: R,
    sink: Arc<Mutex<Vec<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            sink.lock().unwrap().push(line);
        }
    })
}

fn await_server_log(logs: &Arc<Mutex<Vec<String>>>, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if logs.lock().unwrap().iter().any(|l| l.contains(needle)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "timed out waiting for '{needle}' in server logs:\n{}",
        logs.lock().unwrap().join("\n")
    );
}

fn toml_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy().into_owned()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worldedit_console_and_player_ingame_edit_smoke() {
    let root = workspace_root();
    let worldedit_jar = root.join("compat").join("worldedit-bukkit-7.4.6-dist.jar");
    if !worldedit_jar.is_file() {
        eprintln!(
            "skipping worldedit_ingame_smoke: {} not present",
            worldedit_jar.display()
        );
        return;
    }

    let java_dir = root.join("java");
    let host_jar = find_jar(&java_dir.join("ferrite-host/build/libs"), "ferrite-host").unwrap();
    let api_jar = find_jar(
        &java_dir.join("ferrite-api-impl/build/libs"),
        "ferrite-api-impl",
    )
    .unwrap();
    let nms_jar = find_jar(&java_dir.join("ferrite-nms/build/libs"), "ferrite-nms").unwrap();
    let sep = if cfg!(windows) { ';' } else { ':' };
    let api_deps =
        std::fs::read_to_string(java_dir.join("ferrite-api-impl/build/paper-api-classpath.txt"))
            .unwrap()
            .split(sep)
            .filter(|e| !e.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect::<Vec<_>>();
    let mut runtime_classpath = vec![api_jar, nms_jar];
    runtime_classpath.extend(api_deps);

    let dir = tempfile::tempdir().unwrap();
    let plugins_dir = dir.path().join("plugins");
    let data_dir = dir.path().join("plugin-data");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::copy(&worldedit_jar, plugins_dir.join("worldedit-bukkit.jar")).unwrap();

    let port = free_port();
    let runtime_entries = runtime_classpath
        .iter()
        .map(|p| toml_string(p))
        .collect::<Vec<_>>()
        .join(",\n  ");
    let config = format!(
        "bind-address = \"127.0.0.1:{port}\"\n\
         online-mode = false\n\
         generator = \"superflat\"\n\
         gamemode = \"creative\"\n\
         view-distance = 2\n\
         \n\
         [plugins]\n\
         enabled = true\n\
         dir = {plugins}\n\
         data-dir = {data}\n\
         host-jar = {host}\n\
         runtime-classpath = [\n  {runtime_entries}\n]\n\
         api-impl-class = \"dev.ferrite.api.FerriteHostApi\"\n\
         start-timeout-ms = 60000\n\
         call-timeout-ms = 30000\n",
        plugins = toml_string(&plugins_dir),
        data = toml_string(&data_dir),
        host = toml_string(&host_jar),
    );
    std::fs::write(dir.path().join("ferrite.toml"), config).unwrap();
    std::fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ferrite-server"));
    cmd.arg("--headless")
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let vanilla_generated = root.join("vanilla").join("26.3").join("generated");
    if vanilla_generated.is_dir() {
        cmd.env(
            "JAVA_TOOL_OPTIONS",
            format!(
                "-Dferrite.vanilla.generated={}",
                vanilla_generated.display()
            ),
        );
    }
    let mut child = cmd.spawn().unwrap();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let stdout_handle = spawn_log_collector(child.stdout.take().unwrap(), Arc::clone(&logs));
    let stderr_handle = spawn_log_collector(child.stderr.take().unwrap(), Arc::clone(&logs));
    let mut stdin = child.stdin.take().unwrap();
    let mut server = Server(child);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if TcpStream::connect(addr).await.is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "server never listened");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    await_server_log(&logs, "7.4.6+7648-00664b0", Duration::from_secs(20));

    // 1. Console command: `/we version`
    writeln!(stdin, "/we version").unwrap();
    stdin.flush().unwrap();
    await_server_log(&logs, "CONSOLE:", Duration::from_secs(10));

    // 2. Player command path: OP before join, join, //pos1, //pos2, //set minecraft:stone
    writeln!(stdin, "/op WorldEditBot").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(150));

    let mut bot = Bot::join(addr, "WorldEditBot").await;
    let _ = bot.until_spawned(9).await;

    bot.send(&ServerboundChatCommand {
        command: "/pos1 0,65,0".into(),
    })
    .await;
    let pos1_chat = bot
        .drain_chat_until("First position", Duration::from_secs(5))
        .await;
    println!("POS1 CHAT: {pos1_chat:?}");

    bot.send(&ServerboundChatCommand {
        command: "/pos2 1,65,1".into(),
    })
    .await;
    let pos2_chat = bot
        .drain_chat_until("Second position", Duration::from_secs(5))
        .await;
    println!("POS2 CHAT: {pos2_chat:?}");

    bot.send(&ServerboundChatCommand {
        command: "/set minecraft:stone".into(),
    })
    .await;
    let set_chat = bot
        .drain_chat_until("propertiesMap", Duration::from_secs(5))
        .await;

    writeln!(stdin, "stop").unwrap();
    stdin.flush().unwrap();
    let _ = server.0.wait();
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let server_logs = logs.lock().unwrap().join("\n");
    assert!(
        server_logs.contains("WorldEdit version 7.4.6"),
        "missing console /we version output"
    );
    assert!(
        pos1_chat
            .iter()
            .any(|m| m.contains("First position set to (0, 65, 0)")),
        "missing pos1 feedback: {pos1_chat:?}"
    );
    assert!(
        pos2_chat
            .iter()
            .any(|m| m.contains("Second position set to (1, 65, 1) (4)")),
        "missing pos2 feedback: {pos2_chat:?}"
    );
    // //set minecraft:stone reaches WorldEdit's DefaultBlockParser -> BlockType.computeProperties,
    // which is blocked at the exact Paperweight NMS adapter boundary: WorldEditPlugin.getBukkitImplAdapter()
    // is null because v26_3.PaperweightAdapter requires org.bukkit.craftbukkit.CraftServer.
    assert!(
        set_chat.iter().any(|m| m.contains("propertiesMap")),
        "expected reproducible BlockType.computeProperties propertiesMap boundary: {set_chat:?}"
    );
    assert!(
        server_logs.contains("BlockType.computeProperties"),
        "expected BlockType.computeProperties stack trace in server logs"
    );
}
