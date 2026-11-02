//! End-to-end M3: the server starts the embedded JVM, enables plugins, fires their events,
//! and disables them on stop.
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ferrite_protocol::{
    Decode, Encode, FrameDecoder, FrameEncoder, Packet, VarInt, encode_packet,
    packets::{
        configuration::{
            AcknowledgeFinishConfiguration, ClientboundKnownPacks, ServerboundKnownPacks,
        },
        handshake::{Intent, Intention},
        login::{LoginAcknowledged, LoginStart},
        play::{
            BlockUpdate, ChunkBatchReceived, ClientboundPlayerChat, ConfirmTeleport, CreativeSlot,
            ItemStack, LastSeenUpdate, MovePosition, MoveRotation, PlayerAction, PositionSync,
            ServerboundChat, SetExperience, SetHealth, SystemChat, UseItemOn,
        },
    },
    types::BlockPos,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives in <repo>/crates/<name>")
        .to_owned()
}

/// Gradle builds share one daemon and project directory; serialize the test helpers that invoke it.
static GRADLE_LOCK: Mutex<()> = Mutex::new(());

fn snapshot_bridge(source: &Path, directory: &Path) -> PathBuf {
    let target = directory.join(source.file_name().expect("jar filename"));
    fs::copy(source, &target).expect("copy jar into isolated test directory");
    target
}

#[test]
fn bridge_snapshot_survives_a_later_build() {
    let dir = tempfile::tempdir().unwrap();
    let build = dir.path().join("build.jar");
    let runtime = dir.path().join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::write(&build, b"first build").unwrap();
    let snapshot = snapshot_bridge(&build, &runtime);
    fs::write(&build, b"second build").unwrap();
    assert_eq!(fs::read(snapshot).unwrap(), b"first build");
}

/// Build the Java bridge, the test plugin and its dependency with the Gradle wrapper.
fn build_jars(repo: &Path, java_home: &Path, runtime: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let _guard = GRADLE_LOCK.lock().unwrap();
    let project = repo.join("java").join("ferrite-api-impl");
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args([
            "/C",
            ".\\gradlew.bat",
            "--console=plain",
            ":test-plugin:jar",
            ":test-dependency:jar",
            "shadedJar",
        ]);
        command
    } else {
        let mut command = Command::new("./gradlew");
        command.args([
            "--console=plain",
            ":test-plugin:jar",
            ":test-dependency:jar",
            "shadedJar",
        ]);
        command
    };
    let output = command
        .current_dir(&project)
        .env("JAVA_HOME", java_home)
        .output()
        .expect("running the Gradle wrapper");
    assert!(
        output.status.success(),
        "gradle build failed in {}:\n{}\n{}",
        project.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let plugin = repo.join("java/test-plugin/build/libs/test-plugin.jar");
    let dependency = repo.join("java/test-dependency/build/libs/test-dependency.jar");
    let bridge = project.join("build/libs/ferrite-api-impl-all.jar");
    for jar in [&plugin, &dependency, &bridge] {
        assert!(jar.exists(), "missing built jar: {}", jar.display());
    }
    (
        snapshot_bridge(&plugin, runtime),
        snapshot_bridge(&dependency, runtime),
        snapshot_bridge(&bridge, runtime),
    )
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("binding a probe port")
        .local_addr()
        .expect("probe address")
        .port()
}

/// Kills the server if a test assertion panics before it exits.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn capture(
    mut reader: impl Read + Send + 'static,
) -> (Arc<Mutex<String>>, std::thread::JoinHandle<()>) {
    let log = Arc::new(Mutex::new(String::new()));
    let target = Arc::clone(&log);
    let handle = std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => target
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push_str(&String::from_utf8_lossy(&buffer[..read])),
            }
        }
    });
    (log, handle)
}

fn wait_for_line(
    path: &Path,
    needle: &str,
    stdout: &Arc<Mutex<String>>,
    stderr: &Arc<Mutex<String>>,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(content) = fs::read_to_string(path)
            && content.lines().any(|line| line == needle)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "plugin never wrote {needle:?} to {}:\n{}\n{}\n{}",
            path.display(),
            fs::read_to_string(path).unwrap_or_default(),
            stdout.lock().unwrap(),
            stderr.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A minimal protocol bot: enough to log in, chat, run a command and disconnect.
struct Bot {
    stream: TcpStream,
    encoder: FrameEncoder,
    decoder: FrameDecoder,
}

impl Bot {
    fn join(
        addr: SocketAddr,
        name: &str,
        stdout: &Arc<Mutex<String>>,
        stderr: &Arc<Mutex<String>>,
    ) -> Self {
        let deadline = Instant::now() + Duration::from_secs(60);
        let stream = loop {
            match TcpStream::connect(addr) {
                Ok(stream) => break stream,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => panic!("connecting to {addr}: {error}"),
            }
        };
        // The first response waits for the whole world startup (vanilla registry and
        // recipe preload), which is minutes in a debug build on a loaded machine. Every
        // plugin deadline below keeps its own 60 s budget.
        stream
            .set_read_timeout(Some(Duration::from_secs(300)))
            .unwrap();
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
        });
        bot.send(&LoginStart {
            name: name.into(),
            uuid: uuid::Uuid::nil(),
        });
        let first = match bot.read_frame() {
            Some(frame) => frame,
            None => panic!(
                "no set compression packet:\n{}\n{}",
                stdout.lock().unwrap(),
                stderr.lock().unwrap()
            ),
        };
        assert_eq!(VarInt::decode(&mut first.as_slice()).unwrap().0, 3);
        bot.encoder.set_compression(Some(256), 6);
        bot.decoder.set_compression(Some(256));
        let login = match bot.read_frame() {
            Some(frame) => frame,
            None => panic!(
                "no login success packet:\n{}\n{}",
                stdout.lock().unwrap(),
                stderr.lock().unwrap()
            ),
        };
        assert_eq!(VarInt::decode(&mut login.as_slice()).unwrap().0, 2);
        bot.send(&LoginAcknowledged);
        loop {
            let frame = match bot.read_frame() {
                Some(frame) => frame,
                None => panic!(
                    "no finish configuration packet:\n{}\n{}",
                    stdout.lock().unwrap(),
                    stderr.lock().unwrap()
                ),
            };
            let mut body = frame.as_slice();
            let id = VarInt::decode(&mut body).unwrap().0;
            if id == ClientboundKnownPacks::ID {
                let packs = ClientboundKnownPacks::decode(&mut body).unwrap();
                bot.send(&ServerboundKnownPacks { packs: packs.packs });
            }
            if id == 3 {
                break;
            }
        }
        bot.send(&AcknowledgeFinishConfiguration);
        bot.until(73);
        bot.stream
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        bot
    }

    fn send<P: Packet>(&mut self, packet: &P) {
        let mut body = Vec::new();
        encode_packet(packet, &mut body);
        let mut wire = Vec::new();
        self.encoder.encode(&body, &mut wire);
        self.stream.write_all(&wire).unwrap();
    }

    /// Sends an unsigned chat message (an offline-mode client's packet shape).
    fn chat(&mut self, text: &str) {
        self.send(&ServerboundChat {
            message: text.into(),
            timestamp: 1_700_000_000_000,
            salt: 0,
            signature: None,
            last_seen: LastSeenUpdate::new(0, &[], 0),
        });
    }

    fn raw(&mut self, id: i32, text: &str) {
        let mut body = Vec::new();
        VarInt(id).encode(&mut body);
        text.encode(&mut body);
        let mut wire = Vec::new();
        self.encoder.encode(&body, &mut wire);
        self.stream.write_all(&wire).unwrap();
    }

    fn read_frame(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(frame) = self.decoder.next_frame().unwrap() {
                return Some(frame);
            }
            let mut bytes = [0_u8; 8192];
            match self.stream.read(&mut bytes) {
                Ok(0) => return None,
                Ok(read) => self.decoder.feed(&bytes[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return None;
                }
                Err(error) => panic!("reading from the server: {error}"),
            }
        }
    }

    fn next_play(&mut self) -> Option<(i32, Vec<u8>)> {
        let bytes = self.read_frame()?;
        let mut body = bytes.as_slice();
        let id = VarInt::decode(&mut body).unwrap().0;
        let rest = body.to_vec();
        match id {
            73 => {
                let mut body = rest.as_slice();
                let position = PositionSync::decode(&mut body).unwrap();
                self.send(&ConfirmTeleport {
                    id: position.teleport_id,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    yaw: position.yaw,
                    pitch: position.pitch,
                });
            }
            11 => self.send(&ChunkBatchReceived {
                chunks_per_tick: 64.0,
            }),
            _ => {}
        }
        Some((id, rest))
    }

    fn until(&mut self, wanted: i32) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for packet {wanted}"
            );
            if let Some((id, body)) = self.next_play()
                && id == wanted
            {
                return body;
            }
        }
    }

    fn move_to(&mut self, x: f64, y: f64, z: f64) {
        self.send(&MovePosition { x, y, z, flags: 1 });
    }

    fn collect_chats(&mut self, duration: Duration) -> Vec<String> {
        let deadline = Instant::now() + duration;
        let mut messages = Vec::new();
        while Instant::now() < deadline {
            if let Some((id, body)) = self.next_play()
                && let Some(text) = chat_text(id, &body)
            {
                messages.push(text);
            }
        }
        messages
    }
}

/// Chat reaches the client as `player_chat` (signed) or `system_chat` (plain).
fn chat_text(id: i32, mut body: &[u8]) -> Option<String> {
    match id {
        124 => Some(SystemChat::decode(&mut body).unwrap().content.0),
        66 => Some(
            ClientboundPlayerChat::decode(&mut body)
                .unwrap()
                .body
                .content,
        ),
        _ => None,
    }
}

fn wait_for_prefix(
    path: &Path,
    prefix: &str,
    stdout: &Arc<Mutex<String>>,
    stderr: &Arc<Mutex<String>>,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(content) = fs::read_to_string(path)
            && content.lines().any(|line| line.starts_with(prefix))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "plugin never wrote a {prefix:?} line to {}:\n{}\n{}\n{}",
            path.display(),
            fs::read_to_string(path).unwrap_or_default(),
            stdout.lock().unwrap(),
            stderr.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_block_state(
    bot: &mut Bot,
    position: BlockPos,
    stdout: &Arc<Mutex<String>>,
    stderr: &Arc<Mutex<String>>,
    events: &Path,
) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "no block update for {:?}\nevents:\n{}\n{}\n{}",
            position,
            fs::read_to_string(events).unwrap_or_default(),
            stdout.lock().unwrap(),
            stderr.lock().unwrap()
        );
        if let Some((id, body)) = bot.next_play()
            && id == BlockUpdate::ID
        {
            let update = BlockUpdate::decode(&mut body.as_slice()).unwrap();
            if BlockPos::unpack(update.location) == position {
                return update.block_state.0;
            }
        }
    }
}

fn wait_for_chat(
    bot: &mut Bot,
    needle: &str,
    stdout: &Arc<Mutex<String>>,
    stderr: &Arc<Mutex<String>>,
    events: &Path,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    loop {
        if Instant::now() >= deadline {
            panic!(
                "no system chat containing {needle:?}; saw {seen:?}\nevents:\n{}\n{}\n{}",
                fs::read_to_string(events).unwrap_or_default(),
                stdout.lock().unwrap(),
                stderr.lock().unwrap()
            );
        }
        if let Some((id, body)) = bot.next_play()
            && let Some(message) = chat_text(id, &body)
        {
            if message.contains(needle) {
                return message;
            }
            seen.push(message);
        }
    }
}

#[test]
fn server_loads_plugins_at_startup_and_disables_them_on_stop() {
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping plugin lifecycle test: {error}");
            return;
        }
    };
    let repo = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let (plugin_jar, dependency_jar, bridge_jar) = build_jars(&repo, &java_home, dir.path());
    let plugins = dir.path().join("plugins");
    fs::create_dir(&plugins).unwrap();
    fs::copy(plugin_jar, plugins.join("test-plugin.jar")).unwrap();
    fs::copy(dependency_jar, plugins.join("test-dependency.jar")).unwrap();
    let port = free_port();
    fs::write(
        dir.path().join("ferrite.toml"),
        format!(
            "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nview-distance = 2\nmotd = \"Plugin lifecycle\"\ngenerator = \"superflat\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
            toml_path(&bridge_jar)
        ),
    )
    .unwrap();
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
            .current_dir(dir.path())
            .env("JAVA_HOME", &java_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("starting the server"),
    );
    let (stdout, stdout_reader) = capture(process.0.stdout.take().unwrap());
    let (stderr, stderr_reader) = capture(process.0.stderr.take().unwrap());
    let enabled = plugins.join("TestPlugin/enabled.txt");
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if enabled.exists() {
            break;
        }
        if let Some(status) = process.0.try_wait().unwrap() {
            panic!(
                "server exited before the plugin enabled ({status}):\n{}\n{}",
                stdout.lock().unwrap().clone(),
                stderr.lock().unwrap().clone()
            );
        }
        if Instant::now() >= deadline {
            panic!(
                "plugin did not write {} within 180 seconds:\n{}\n{}",
                enabled.display(),
                stdout.lock().unwrap().clone(),
                stderr.lock().unwrap().clone()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let events = plugins.join("TestPlugin/events.txt");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut bot = Bot::join(addr, "Bot", &stdout, &stderr);
    wait_for_line(&events, "join Bot", &stdout, &stderr);
    wait_for_prefix(&events, "chunk-load ", &stdout, &stderr);
    bot.chat("hello plugins");
    wait_for_line(&events, "chat hello plugins", &stdout, &stderr);
    wait_for_line(&events, "legacy-chat hello plugins", &stdout, &stderr);
    let broadcast = wait_for_chat(&mut bot, "hello plugins", &stdout, &stderr, &events);
    assert!(broadcast.contains("hello plugins"), "{broadcast}");
    bot.chat("cancel this");
    wait_for_line(&events, "chat cancel this", &stdout, &stderr);
    let after_cancel = bot.collect_chats(Duration::from_millis(1500));
    assert!(
        !after_cancel
            .iter()
            .any(|message| message.contains("cancel this")),
        "a cancelled chat event was still broadcast: {after_cancel:?}"
    );
    bot.raw(7, "testping");
    let pong = wait_for_chat(&mut bot, "pong", &stdout, &stderr, &events);
    assert_eq!(pong, "pong");
    wait_for_prefix(&events, "command /testping", &stdout, &stderr);
    bot.move_to(2.5, 65.0, 0.5);
    wait_for_prefix(&events, "move Bot 2 65 0", &stdout, &stderr);
    bot.move_to(2.5, 100.0, 0.5);
    wait_for_prefix(&events, "move-cancel Bot", &stdout, &stderr);
    let rejected = PositionSync::decode(&mut bot.until(73).as_slice()).unwrap();
    assert_eq!((rejected.x, rejected.y, rejected.z), (2.5, 65.0, 0.5));
    bot.move_to(0.5, 65.0, 0.5);
    wait_for_prefix(&events, "move Bot 0 65 0", &stdout, &stderr);
    bot.send(&MoveRotation {
        yaw: 0.0,
        pitch: 90.0,
        flags: 1,
    });
    bot.send(&PlayerAction {
        status: VarInt(0),
        location: BlockPos::new(0, 64, 0).pack(),
        face: 1,
        sequence: VarInt(30),
    });
    wait_for_prefix(&events, "interact left 0 64 0", &stdout, &stderr);
    bot.send(&UseItemOn {
        hand: VarInt(0),
        location: BlockPos::new(0, 63, 0).pack(),
        face: VarInt(1),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(31),
    });
    wait_for_prefix(&events, "interact right 0 63 0", &stdout, &stderr);
    wait_for_prefix(&events, "break 0 64 0", &stdout, &stderr);
    wait_for_line(
        &events,
        "break-type minecraft:grass_block world",
        &stdout,
        &stderr,
    );
    assert_eq!(
        wait_for_block_state(&mut bot, BlockPos::new(0, 64, 0), &stdout, &stderr, &events),
        0,
        "a creative break did not clear the block"
    );
    bot.move_to(1.5, 65.0, 0.5);
    wait_for_prefix(&events, "move Bot 1 65 0", &stdout, &stderr);
    bot.send(&PlayerAction {
        status: VarInt(0),
        location: BlockPos::new(1, 64, 0).pack(),
        face: 1,
        sequence: VarInt(32),
    });
    wait_for_prefix(&events, "break-cancel 1 64 0", &stdout, &stderr);
    let grass = i32::from(ferrite_data::blocks::default_state("grass_block").unwrap());
    assert_eq!(
        wait_for_block_state(&mut bot, BlockPos::new(1, 64, 0), &stdout, &stderr, &events),
        grass,
        "a cancelled break did not restore the block"
    );
    bot.move_to(0.5, 65.0, 0.5);
    wait_for_prefix(&events, "move Bot 0 65 0", &stdout, &stderr);
    bot.send(&CreativeSlot {
        slot: 36,
        item: ItemStack {
            count: 1,
            item_id: ferrite_data::registries::id("minecraft:item", "minecraft:stone").unwrap(),
        },
    });
    bot.send(&UseItemOn {
        hand: VarInt(0),
        location: BlockPos::new(0, 63, 0).pack(),
        face: VarInt(1),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(33),
    });
    wait_for_prefix(&events, "place-cancel 0 64 0", &stdout, &stderr);
    assert_eq!(
        wait_for_block_state(&mut bot, BlockPos::new(0, 64, 0), &stdout, &stderr, &events),
        0,
        "a cancelled placement was not rolled back on the client"
    );
    bot.raw(7, "testping nope");
    wait_for_prefix(&events, "command /testping nope", &stdout, &stderr);
    let after_command_cancel = bot.collect_chats(Duration::from_millis(1200));
    assert!(
        !after_command_cancel
            .iter()
            .any(|message| message.contains("pong")),
        "a cancelled command still ran: {after_command_cancel:?}"
    );
    drop(bot);
    wait_for_line(&events, "quit Bot", &stdout, &stderr);

    let mut stdin = process.0.stdin.take().unwrap();
    stdin.write_all(b"stop\n").unwrap();
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            panic!("server did not stop within 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    stdout_reader.join().unwrap();
    stderr_reader.join().unwrap();
    let logs = format!(
        "{}\n{}",
        stdout.lock().unwrap().clone(),
        stderr.lock().unwrap().clone()
    );
    assert!(status.success(), "server exited with {status}:\n{logs}");
    assert!(
        logs.contains("server name Ferrite"),
        "plugin did not see the Ferrite server:\n{logs}"
    );
    assert!(
        logs.contains("FerriteTest disabled"),
        "plugin onDisable did not run:\n{logs}"
    );
    assert!(
        plugins.join("TestPlugin/disabled.txt").exists(),
        "plugin did not write its disable marker:\n{logs}"
    );
}

/// Build the typical-plugin fixture together with the shaded bridge.
fn build_typical_jar(repo: &Path, java_home: &Path, runtime: &Path) -> (PathBuf, PathBuf) {
    let _guard = GRADLE_LOCK.lock().unwrap();
    let project = repo.join("java").join("ferrite-api-impl");
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args([
            "/C",
            r".\gradlew.bat",
            "--console=plain",
            ":test-plugin-typical:jar",
            "shadedJar",
        ]);
        command
    } else {
        let mut command = Command::new("./gradlew");
        command.args(["--console=plain", ":test-plugin-typical:jar", "shadedJar"]);
        command
    };
    let output = command
        .current_dir(&project)
        .env("JAVA_HOME", java_home)
        .output()
        .expect("running the Gradle wrapper");
    assert!(
        output.status.success(),
        "gradle typical plugin build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let jar = repo.join("java/test-plugin-typical/build/libs/test-plugin-typical.jar");
    assert!(jar.exists(), "missing built jar: {}", jar.display());
    let bridge = project.join("build/libs/ferrite-api-impl-all.jar");
    (
        snapshot_bridge(&jar, runtime),
        snapshot_bridge(&bridge, runtime),
    )
}

/// A plugin shaped like a real one: config.yml, a command with tab completion, a join listener
/// and a repeating task that cancels itself.
#[test]
fn typical_plugin_uses_config_commands_listener_and_repeating_task() {
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping typical plugin test: {error}");
            return;
        }
    };
    let repo = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let (plugin_jar, bridge_jar) = build_typical_jar(&repo, &java_home, dir.path());
    let plugins = dir.path().join("plugins");
    fs::create_dir(&plugins).unwrap();
    fs::copy(plugin_jar, plugins.join("typical-plugin.jar")).unwrap();
    let port = free_port();
    fs::write(
        dir.path().join("ferrite.toml"),
        format!(
            "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nview-distance = 2\ngenerator = \"superflat\"\nmotd = \"Typical plugin\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
            toml_path(&bridge_jar)
        ),
    )
    .unwrap();
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
            .current_dir(dir.path())
            .env("JAVA_HOME", &java_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("starting the server"),
    );
    let (stdout, stdout_reader) = capture(process.0.stdout.take().unwrap());
    let (stderr, stderr_reader) = capture(process.0.stderr.take().unwrap());
    let folder = plugins.join("FerriteTypical");
    let enabled = folder.join("enabled.txt");
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if enabled.exists() {
            break;
        }
        if let Some(status) = process.0.try_wait().unwrap() {
            panic!(
                "server exited before the typical plugin enabled ({status}):\n{}\n{}",
                stdout.lock().unwrap().clone(),
                stderr.lock().unwrap().clone()
            );
        }
        if Instant::now() >= deadline {
            panic!(
                "typical plugin did not write {} within 180 seconds:\n{}\n{}",
                enabled.display(),
                stdout.lock().unwrap().clone(),
                stderr.lock().unwrap().clone()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let config = fs::read_to_string(folder.join("config.txt")).unwrap();
    assert_eq!(
        config.trim(),
        "hello from config|2|DIAMOND",
        "config.yml was not loaded"
    );
    let events = folder.join("events.txt");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut bot = Bot::join(addr, "Typical", &stdout, &stderr);
    wait_for_prefix(&events, "join Typical", &stdout, &stderr);
    let joined = fs::read_to_string(&events).unwrap();
    assert!(
        joined.contains("join Typical CREATIVE true"),
        "the plugin.yml default permission was not granted: {joined}"
    );
    wait_for_line(&events, "tab [ping, where, info, give]", &stdout, &stderr);
    // Server surface: the host's real bind address and settings, the vanilla recipe and tag
    // tables loaded from the generated data pack, and the boss-bar registry.
    bot.raw(7, "typical surface");
    let dir_name = dir.path().file_name().unwrap().to_string_lossy();
    wait_for_line(
        &events,
        &format!("ip 127.0.0.1 {port} 2 10 20"),
        &stdout,
        &stderr,
    );
    wait_for_line(
        &events,
        &format!("container {dir_name} update update"),
        &stdout,
        &stderr,
    );
    wait_for_line(&events, "recipes 1763 stick STICK 4 true", &stdout, &stderr);
    wait_for_line(&events, "for-stick 2 true", &stdout, &stderr);
    wait_for_line(
        &events,
        "tags coals true diamond false logs true",
        &stdout,
        &stderr,
    );
    wait_for_line(&events, "boss true 0.25 Boss true true", &stdout, &stderr);
    // On-demand world reads: the superflat test world has grass at y=64 over dirt and air
    // above it, full sky light in the open column, and the plains biome.
    bot.raw(7, "typical world");
    wait_for_line(&events, "block GRASS_BLOCK", &stdout, &stderr);
    wait_for_line(&events, "air AIR sky 15 dark 0", &stdout, &stderr);
    wait_for_line(
        &events,
        "highest 64 biome minecraft:plains",
        &stdout,
        &stderr,
    );
    // Health, food and experience writes reach the region player and are sent to the client.
    bot.raw(7, "typical state");
    wait_for_line(&events, "state 7.5 11 12 0.25 123", &stdout, &stderr);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let packet = SetHealth::decode(&mut bot.until(106).as_slice()).unwrap();
        if (packet.health - 7.5).abs() < 1e-6 && packet.food.0 == 11 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no health packet with the plugin's value"
        );
    }
    loop {
        let packet = SetExperience::decode(&mut bot.until(105).as_slice()).unwrap();
        if packet.level.0 == 12 && packet.total.0 == 123 && (packet.progress - 0.25).abs() < 1e-6 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no experience packet with the plugin's value"
        );
    }
    bot.raw(7, "typical tp");
    wait_for_line(&events, "tp true 2.5 70.0 2.5", &stdout, &stderr);
    loop {
        let packet = PositionSync::decode(&mut bot.until(73).as_slice()).unwrap();
        if (packet.x - 2.5).abs() < 1e-9 && (packet.z - 2.5).abs() < 1e-9 {
            assert!(
                (packet.y - 70.0).abs() < 1e-9,
                "teleported to y={}",
                packet.y
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no teleport to the plugin's location"
        );
    }
    // Published before onEnable, so the fixture can target it with Paper's region scheduler.
    wait_for_line(&events, "region task false", &stdout, &stderr);
    // The Bukkit scheduler runs immediate, delayed and asynchronous tasks.
    wait_for_line(&events, "scheduler task", &stdout, &stderr);
    wait_for_line(&events, "later task", &stdout, &stderr);
    wait_for_line(&events, "async task", &stdout, &stderr);
    wait_for_line(&events, "later async task", &stdout, &stderr);
    wait_for_line(&events, "timer async task", &stdout, &stderr);
    // A repeating async task cancelled from the sync thread must stop running.
    wait_for_line(&events, "async cancelled", &stdout, &stderr);
    std::thread::sleep(Duration::from_millis(300));
    let settled = fs::read_to_string(&events).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fs::read_to_string(&events).unwrap(),
        settled,
        "an async timer kept running after it was cancelled"
    );
    wait_for_line(&events, "task 1", &stdout, &stderr);
    wait_for_line(&events, "task 3", &stdout, &stderr);
    assert!(
        folder.join("task.txt").exists(),
        "the repeating task never reached tick 3"
    );
    wait_for_line(&events, "task 5", &stdout, &stderr);
    assert!(
        folder.join("cancelled.txt").exists(),
        "the repeating task was not cancelled"
    );

    bot.raw(7, "typical ping");
    assert_eq!(
        wait_for_chat(&mut bot, "pong Typical", &stdout, &stderr, &events),
        "pong Typical"
    );
    bot.move_to(5.5, 65.0, 5.5);
    std::thread::sleep(Duration::from_millis(400));
    bot.raw(7, "typical where");
    let location = wait_for_chat(&mut bot, "where world", &stdout, &stderr, &events);
    assert_eq!(
        location, "where world 5 65 5",
        "the plugin did not see the moved player"
    );
    bot.raw(7, "typical give");
    // `PlayerInventory#setItem` goes through `Native.playerInventory`; the game loop routes it
    // to the region owning the player and syncs the slot back to the Bukkit inventory.
    assert_eq!(
        wait_for_chat(
            &mut bot,
            "gave minecraft:diamond",
            &stdout,
            &stderr,
            &events
        ),
        "gave minecraft:diamond x2"
    );
    wait_for_line(&events, "give DIAMOND 2", &stdout, &stderr);
    bot.raw(7, "typical info");
    let info = wait_for_chat(&mut bot, "info world", &stdout, &stderr, &events);
    assert!(
        info.contains("spawn ") && info.contains("players 1"),
        "{info}"
    );

    drop(bot);
    let mut stdin = process.0.stdin.take().unwrap();
    stdin.write_all(b"stop\n").unwrap();
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            panic!("server did not stop within 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    stdout_reader.join().unwrap();
    stderr_reader.join().unwrap();
    let logs = format!(
        "{}\n{}",
        stdout.lock().unwrap().clone(),
        stderr.lock().unwrap().clone()
    );
    assert!(status.success(), "server exited with {status}:\n{logs}");
    assert!(
        logs.contains("FerriteTypical enabled: hello from config"),
        "plugin did not read its config:\n{logs}"
    );
    assert!(
        logs.contains("FerriteTypical disabled"),
        "plugin onDisable did not run:\n{logs}"
    );
    assert!(
        folder.join("disabled.txt").exists(),
        "plugin did not write its disable marker:\n{logs}"
    );
}
