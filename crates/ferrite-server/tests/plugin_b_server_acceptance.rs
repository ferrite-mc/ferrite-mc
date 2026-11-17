//! End-to-end production server-process acceptance test for Plugin B.
//!
//! Starts the real `ferrite-server` binary with `[plugins] enabled = true`, loads both
//! `typical-plugin` and `nms-plugin` into the same embedded JVM via `FerriteHostApi`, drives a
//! protocol bot through join, block break/interact, command dispatch, scheduler ticks, and quit,
//! and verifies clean shutdown and plugin disablement.

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
            ChunkBatchReceived, ClientKeepAlive, ConfirmTeleport, MoveRotation, PlayKeepAlive,
            PlayerAction, PositionSync, UseItemOn,
        },
    },
    types::BlockPos,
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
            // The fixture also builds a paper-plugin.yml descriptor variant.
            // This acceptance test needs the plugin.yml artifact (commands and
            // permissions), and the two jars can be written in either order.
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

fn ensure_java_artifacts(root: &Path) -> (PathBuf, Vec<PathBuf>, PathBuf, PathBuf) {
    let java_dir = root.join("java");
    let classpath_file = java_dir.join("ferrite-api-impl/build/paper-api-classpath.txt");

    let need_build = find_jar(&java_dir.join("ferrite-host/build/libs"), "ferrite-host").is_none()
        || find_jar(
            &java_dir.join("ferrite-api-impl/build/libs"),
            "ferrite-api-impl",
        )
        .is_none()
        || find_jar(&java_dir.join("ferrite-nms/build/libs"), "ferrite-nms").is_none()
        || find_jar(
            &java_dir.join("fixtures/typical-plugin/build/libs"),
            "typical-plugin",
        )
        .is_none()
        || find_jar(
            &java_dir.join("fixtures/nms-plugin/build/libs"),
            "nms-plugin",
        )
        .is_none()
        || !classpath_file.is_file();

    if need_build {
        let gradlew = if cfg!(windows) {
            java_dir.join("gradlew.bat")
        } else {
            java_dir.join("gradlew")
        };
        let status = Command::new(gradlew)
            .args([
                ":ferrite-host:jar",
                ":ferrite-api-impl:jar",
                ":ferrite-api-impl:printApiClasspath",
                ":ferrite-nms:jar",
                ":fixtures:typical-plugin:jar",
                ":fixtures:nms-plugin:jar",
                "--no-daemon",
            ])
            .current_dir(&java_dir)
            .status()
            .expect("running gradlew to build Plugin B jars");
        assert!(status.success(), "gradlew failed with {status}");
    }

    let host_jar =
        find_jar(&java_dir.join("ferrite-host/build/libs"), "ferrite-host").expect("host jar");
    let api_impl_jar = find_jar(
        &java_dir.join("ferrite-api-impl/build/libs"),
        "ferrite-api-impl",
    )
    .expect("api-impl jar");
    let nms_jar =
        find_jar(&java_dir.join("ferrite-nms/build/libs"), "ferrite-nms").expect("nms core jar");
    let typical_jar = find_jar(
        &java_dir.join("fixtures/typical-plugin/build/libs"),
        "typical-plugin",
    )
    .expect("typical-plugin jar");
    let nms_plugin_jar = find_jar(
        &java_dir.join("fixtures/nms-plugin/build/libs"),
        "nms-plugin",
    )
    .expect("nms-plugin jar");

    let sep = if cfg!(windows) { ';' } else { ':' };
    let api_deps = std::fs::read_to_string(&classpath_file)
        .expect("reading paper-api-classpath.txt")
        .split(sep)
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();

    // Order matters: ferrite-api-impl shadows org.bukkit.inventory.ItemStack ahead of paper-api.
    let mut runtime_classpath = vec![api_impl_jar, nms_jar];
    runtime_classpath.extend(api_deps);

    (host_jar, runtime_classpath, typical_jar, nms_plugin_jar)
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

fn await_log_line(path: &Path, needle: &str, timeout: Duration, logs: &Arc<Mutex<Vec<String>>>) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(path)
            && contents.contains(needle)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let contents = std::fs::read_to_string(path).unwrap_or_default();
    let server_logs = logs.lock().unwrap().join("\n");
    panic!(
        "timed out waiting for '{needle}' in {}\n--- typical.log ---\n{contents}\n--- server logs ---\n{server_logs}",
        path.display()
    );
}

fn toml_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy().into_owned()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shipped_ferrite_server_runs_typical_and_nms_fixtures_end_to_end() {
    let root = workspace_root();
    let (host_jar, runtime_classpath, typical_jar, nms_plugin_jar) = ensure_java_artifacts(&root);

    let dir = tempfile::tempdir().expect("temp server dir");
    let plugins_dir = dir.path().join("plugins");
    let data_dir = dir.path().join("plugin-data");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::copy(&typical_jar, plugins_dir.join("typical-plugin.jar")).unwrap();
    std::fs::copy(&nms_plugin_jar, plugins_dir.join("nms-plugin.jar")).unwrap();

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
         call-timeout-ms = 15000\n",
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
    let mut child = cmd.spawn().expect("spawning ferrite-server");

    let logs = Arc::new(Mutex::new(Vec::new()));
    let stdout_handle = spawn_log_collector(child.stdout.take().unwrap(), Arc::clone(&logs));
    let stderr_handle = spawn_log_collector(child.stderr.take().unwrap(), Arc::clone(&logs));
    let mut stdin = child.stdin.take().unwrap();
    let mut server = Server(child);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let deadline = Instant::now() + Duration::from_secs(120);
    let stream = loop {
        if let Ok(stream) = TcpStream::connect(addr).await {
            break stream;
        }
        if let Some(status) = server.0.try_wait().unwrap() {
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            panic!(
                "ferrite-server exited early with {status}\n{}",
                logs.lock().unwrap().join("\n")
            );
        }
        assert!(Instant::now() < deadline, "server never listened");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    drop(stream);

    let typical_log = data_dir.join("TypicalFixture").join("typical.log");
    await_log_line(
        &typical_log,
        "lifecycle:onEnable:done",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "item:unbreakable=true",
        Duration::from_secs(5),
        &logs,
    );
    await_log_line(
        &typical_log,
        "item:flags=[HIDE_UNBREAKABLE]",
        Duration::from_secs(5),
        &logs,
    );
    await_log_line(
        &typical_log,
        "item:pdc=pdc-value",
        Duration::from_secs(5),
        &logs,
    );

    let nms_log = data_dir.join("FerriteNmsFixture").join("acceptance.log");
    await_log_line(&nms_log, "nms-enable:ok", Duration::from_secs(15), &logs);

    let mut bot = Bot::join(addr, "AcceptanceBot").await;
    let (sx, sy, sz) = tokio::time::timeout(Duration::from_secs(60), bot.until_spawned(9))
        .await
        .expect("bot spawned with chunks");

    await_log_line(
        &typical_log,
        "join:lowest:AcceptanceBot",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &nms_log,
        "nms-join:AcceptanceBot:equip=minecraft:diamond_sword,minecraft:iron_helmet:name=AcceptanceGuard:pose=STANDING",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "inventory:main-hand=Inventory Blade:lore=[inv lore]:damage=4:pdc=77",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "inventory:copy-isolated=true",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "inventory:add-remove=addOverflow=0:removeOverflow=0:remaining=3",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "gui:rollback=true:committed=true:reopen-closed=true",
        Duration::from_secs(15),
        &logs,
    );

    // Dispatch a plugin command through the server console.
    writeln!(stdin, "/ferrite-hello Bob").unwrap();
    stdin.flush().unwrap();
    await_log_line(
        &typical_log,
        "command:executed:label=ferrite-hello:sender=CONSOLE:args=Bob",
        Duration::from_secs(15),
        &logs,
    );

    // Look straight down at the block under the bot's feet, then break and interact.
    bot.send(&MoveRotation {
        yaw: 0.0,
        pitch: 90.0,
        flags: 1,
    })
    .await;
    let bx = sx.floor() as i32;
    let by = sy.floor() as i32 - 1;
    let bz = sz.floor() as i32;
    bot.send(&PlayerAction {
        status: VarInt(0),
        location: BlockPos::new(bx, by, bz).pack(),
        face: 1,
        sequence: VarInt(1),
    })
    .await;
    bot.send(&UseItemOn {
        hand: VarInt(0),
        location: BlockPos::new(bx, by - 1, bz).pack(),
        face: VarInt(1),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(2),
    })
    .await;

    await_log_line(
        &typical_log,
        "interact:seen:LEFT_CLICK_BLOCK",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "break:allowed:",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "interact:seen:RIGHT_CLICK_BLOCK",
        Duration::from_secs(15),
        &logs,
    );

    // Verify sync repeating, sync delayed, and async scheduler tasks all run.
    await_log_line(
        &typical_log,
        "scheduler:sync-repeat:1",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "scheduler:sync-repeat:2",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "scheduler:sync-delayed",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &typical_log,
        "scheduler:async:thread=",
        Duration::from_secs(15),
        &logs,
    );

    // Disconnect the bot and verify player_quit fires.
    drop(bot);
    await_log_line(
        &typical_log,
        "quit:AcceptanceBot",
        Duration::from_secs(15),
        &logs,
    );
    await_log_line(
        &nms_log,
        "nms-quit:AcceptanceBot",
        Duration::from_secs(15),
        &logs,
    );

    // Cleanly stop the server and verify both plugins disabled and cleaned up.
    writeln!(stdin, "/stop").unwrap();
    stdin.flush().unwrap();
    drop(stdin);

    let stop_deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < stop_deadline, "server did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    let server_output = logs.lock().unwrap().join("\n");
    assert!(
        status.success(),
        "ferrite-server exited with {status}\n{server_output}"
    );
    await_log_line(
        &typical_log,
        "lifecycle:onDisable:syncRuns=",
        Duration::from_secs(1),
        &logs,
    );
    await_log_line(&nms_log, "nms-disable:ok", Duration::from_secs(1), &logs);

    assert!(
        server_output.contains("TypicalFixture") && server_output.contains("FerriteNmsFixture"),
        "expected both plugins in server logs:\n{server_output}"
    );
    assert!(
        server_output.contains("Ferrite NMS fixture: ok"),
        "expected NMS fixture ok in server logs:\n{server_output}"
    );
    assert!(
        server_output.contains("Ferrite NMS fixture disabled; last result: ok"),
        "expected NMS fixture clean disable in server logs:\n{server_output}"
    );
    assert!(
        server_output.contains("plugins disabled"),
        "expected clean plugin shutdown log:\n{server_output}"
    );
}
