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
            BlockUpdate, BossBar, BossBarAction, ChunkBatchReceived, ClientboundPlayerChat,
            ConfirmTeleport, ContainerClick, CreativeSlot, ItemStack, LastSeenUpdate, MovePosition,
            MoveRotation, OpenScreen, PlayerAction, PlayerCombatKill, PositionSync, Respawn,
            ServerboundChat, ServerboundClientCommand, ServerboundContainerClose, SetExperience,
            SetHealth, SetPlayerInventory, SystemChat, UseItemOn,
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

#[cfg(windows)]
fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrite-server"));
    command.arg("--headless");
    command
}

#[cfg(not(windows))]
fn server_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
}

/// Gradle builds share one daemon and project directory; serialize the test helpers that invoke it.
static GRADLE_LOCK: Mutex<()> = Mutex::new(());

/// CI builds the bridge and fixture jars once before `cargo test`. Test binaries run in
/// parallel, so letting each one invoke Gradle would serialize on the same project directory.
fn java_prebuilt() -> bool {
    std::env::var_os("FERRITE_JAVA_PREBUILT").is_some()
}

/// A Gradle invocation in `java/ferrite-api-impl`. CI installs the pinned Gradle and sets
/// `FERRITE_USE_SYSTEM_GRADLE`, because the wrapper would download that same distribution a
/// second time and that download failed transiently with HTTP 504 in CI.
fn gradle(project: &Path, java_home: &Path, args: &[&str]) -> std::process::Output {
    let use_system_gradle = std::env::var_os("FERRITE_USE_SYSTEM_GRADLE").is_some();
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args([
            "/C",
            if use_system_gradle {
                "gradle"
            } else {
                ".\\gradlew.bat"
            },
        ]);
        command
    } else {
        Command::new(if use_system_gradle {
            "gradle"
        } else {
            "./gradlew"
        })
    };
    command
        .args(args)
        .current_dir(project)
        .env("JAVA_HOME", java_home)
        .output()
        .expect("running Gradle")
}

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
    let project = repo.join("java").join("ferrite-api-impl");
    if !java_prebuilt() {
        let _guard = GRADLE_LOCK.lock().unwrap();
        let output = gradle(
            &project,
            java_home,
            &[
                "--console=plain",
                ":test-plugin:jar",
                ":test-dependency:jar",
                "shadedJar",
            ],
        );
        assert!(
            output.status.success(),
            "gradle build failed in {}:\n{}\n{}",
            project.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
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

fn assert_event_order(path: &Path, first: &str, second: &str) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let position = |prefix: &str| content.lines().position(|line| line.starts_with(prefix));
    let first_at = position(first).unwrap_or_else(|| {
        panic!(
            "plugin never wrote a {first:?} line to {}:\n{content}",
            path.display()
        )
    });
    let second_at = position(second).unwrap_or_else(|| {
        panic!(
            "plugin never wrote a {second:?} line to {}:\n{content}",
            path.display()
        )
    });
    assert!(
        first_at < second_at,
        "expected {first:?} before {second:?} in {}:\n{content}",
        path.display()
    );
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
    // The server refuses to start without `eula=true`; this fixture runs only Ferrite.
    fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();
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
        server_command()
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
    // A plugin-created inventory reaches the client as an open screen. A click and close from
    // the bot must travel back through the same JVM before the server shuts down.
    bot.raw(7, "test-inventory");
    let open = OpenScreen::decode(&mut bot.until(OpenScreen::ID).as_slice()).unwrap();
    wait_for_line(&events, "inventory-open Bot", &stdout, &stderr);
    bot.send(&ContainerClick::<ferrite_item::HashedSlot> {
        window: open.window,
        state: VarInt(0),
        slot: 0,
        button: 0,
        input: VarInt(0),
        changed: Vec::new(),
        carried: None,
    });
    wait_for_line(&events, "click 0", &stdout, &stderr);
    bot.send(&ServerboundContainerClose {
        window: open.window,
    });
    wait_for_line(&events, "close Bot", &stdout, &stderr);
    assert_event_order(&events, "inventory-open Bot", "click 0");
    assert_event_order(&events, "click 0", "close Bot");
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
    assert_event_order(&events, "interact left 0 64 0", "break 0 64 0");
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
    assert_event_order(&events, "interact right 0 63 0", "place 0 64 0");
    assert_event_order(&events, "place 0 64 0", "place-cancel 0 64 0");
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
    // `/kill` goes through the server death path: the plugin sees PlayerDeathEvent and the
    // bot sees the combat-kill screen. The respawn request must produce PlayerRespawnEvent,
    // a new health bar and the quit marker before the server is stopped.
    let mut stdin = process.0.stdin.take().unwrap();
    stdin.write_all(b"kill Bot\n").unwrap();
    let kill = PlayerCombatKill::decode(&mut bot.until(PlayerCombatKill::ID).as_slice()).unwrap();
    assert!(kill.message.0.contains("Bot"), "{kill:?}");
    let dead = SetHealth::decode(&mut bot.until(SetHealth::ID).as_slice()).unwrap();
    assert_eq!(dead.health, 0.0, "the death health bar was not sent");
    wait_for_line(&events, "death Bot", &stdout, &stderr);
    bot.send(&ServerboundClientCommand { action: VarInt(0) });
    let _ = Respawn::decode(&mut bot.until(Respawn::ID).as_slice()).unwrap();
    let alive = SetHealth::decode(&mut bot.until(SetHealth::ID).as_slice()).unwrap();
    assert_eq!((alive.health, alive.food.0), (20.0, 20));
    wait_for_prefix(&events, "respawn ", &stdout, &stderr);
    assert_event_order(&events, "death Bot", "respawn ");
    drop(bot);
    wait_for_line(&events, "quit Bot", &stdout, &stderr);

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
    let project = repo.join("java").join("ferrite-api-impl");
    if !java_prebuilt() {
        let _guard = GRADLE_LOCK.lock().unwrap();
        let output = gradle(
            &project,
            java_home,
            &["--console=plain", ":test-plugin-typical:jar", "shadedJar"],
        );
        assert!(
            output.status.success(),
            "gradle typical plugin build failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let jar = repo.join("java/test-plugin-typical/build/libs/test-plugin-typical.jar");
    assert!(jar.exists(), "missing built jar: {}", jar.display());
    let bridge = project.join("build/libs/ferrite-api-impl-all.jar");
    assert!(bridge.exists(), "missing built jar: {}", bridge.display());
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
    // The server refuses to start without `eula=true`; this fixture runs only Ferrite.
    fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();
    let port = free_port();
    fs::write(
        dir.path().join("ferrite.toml"),
        format!(
            "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nmax-players = 42\nview-distance = 1\ngenerator = \"superflat\"\nmotd = \"Typical plugin\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
            toml_path(&bridge_jar)
        ),
    )
    .unwrap();
    let mut process = Process(
        server_command()
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
    // The config asks for a view distance below the client minimum, so the plugin must see the
    // same effective distance (2) the host sends to clients.
    wait_for_line(
        &events,
        &format!("ip 127.0.0.1 {port} 2 2 42"),
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
    // The same Bukkit bar is a live client bar: /surface added the sender as a viewer before
    // removing the registry entry, so an add packet with the plugin's value must reach the bot.
    let boss_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let packet = BossBar::decode(&mut bot.until(BossBar::ID).as_slice()).unwrap();
        match packet.action {
            BossBarAction::Add { name, progress, .. }
                if name.0 == "Boss" && (progress - 0.25).abs() < f32::EPSILON =>
            {
                break;
            }
            _ => {}
        }
        assert!(
            Instant::now() < boss_deadline,
            "no Bukkit boss bar packet with the plugin's value"
        );
    }
    // removeBossBar must hide the bar again rather than leaving a stale client entry.
    let remove_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let packet = BossBar::decode(&mut bot.until(BossBar::ID).as_slice()).unwrap();
        if matches!(packet.action, BossBarAction::Remove) {
            break;
        }
        assert!(
            Instant::now() < remove_deadline,
            "no Bukkit boss bar remove packet"
        );
    }
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
    // ChunkSnapshot reads the host's biome data: the superflat chunk holds plains and no nether biome.
    wait_for_line(
        &events,
        "snapshot minecraft:plains has-plains true has-nether false",
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

/// A plugin calls the client-visible API (lightning, scoreboard, team, boss bar, equipment,
/// sent chunks); the bot must receive the matching packets over the wire.
#[test]
fn plugin_api_calls_reach_the_client_as_packets() {
    use ferrite_protocol::packets::play::{
        AddEntity, BossBar, BossBarAction, PlayerTeam, SetDisplayObjective, SetEquipment,
        SetObjective, SetScore, SoundEffect, TeamAction,
    };
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping packet verification: {error}");
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
    fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();
    let port = free_port();
    fs::write(
        dir.path().join("ferrite.toml"),
        format!(
            "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nview-distance = 2\nmotd = \"Packet check\"\ngenerator = \"superflat\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
            toml_path(&bridge_jar)
        ),
    )
    .unwrap();
    let mut process = Process(
        server_command()
            .current_dir(dir.path())
            .env("JAVA_HOME", &java_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("starting the server"),
    );
    let (stdout, _stdout_reader) = capture(process.0.stdout.take().unwrap());
    let (stderr, _stderr_reader) = capture(process.0.stderr.take().unwrap());
    let enabled = plugins.join("TestPlugin/enabled.txt");
    let deadline = Instant::now() + Duration::from_secs(180);
    while !enabled.exists() {
        assert!(
            process.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "plugin did not enable:\n{}\n{}",
            stdout.lock().unwrap(),
            stderr.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let events = plugins.join("TestPlugin/events.txt");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut bot = Bot::join(addr, "Bot", &stdout, &stderr);
    wait_for_line(&events, "join Bot", &stdout, &stderr);
    wait_for_prefix(&events, "chunk-load ", &stdout, &stderr);
    bot.raw(7, "verify-features");
    wait_for_prefix(&events, "verify chunk-sent ", &stdout, &stderr);
    let report = fs::read_to_string(&events).unwrap();
    assert!(
        report
            .lines()
            .any(|line| line == "verify chunk-sent true sent-chunks>0 true"),
        "chunks the client was sent were not reported:\n{report}"
    );
    // The plugin writes PDC and item flags through a real inventory slot update; the host must
    // forward that slot to the client with both components intact.
    bot.raw(7, "verify-item");
    wait_for_line(&events, "verify-item set", &stdout, &stderr);
    // A QuickCraft drag (pick up, drag over two slots, release) must reach the plugin as one
    // InventoryDragEvent naming the slots the drag changed.
    bot.raw(7, "drag-setup");
    wait_for_line(&events, "drag-setup", &stdout, &stderr);
    bot.send(&ContainerClick::<ferrite_item::HashedSlot> {
        window: VarInt(0),
        state: VarInt(-1),
        slot: 36,
        button: 0,
        input: VarInt(0),
        changed: Vec::new(),
        carried: None,
    });
    for (slot, button) in [(-999i16, 0i8), (9, 1), (10, 1), (-999, 2)] {
        bot.send(&ContainerClick::<ferrite_item::HashedSlot> {
            window: VarInt(0),
            state: VarInt(-1),
            slot,
            button,
            input: VarInt(5),
            changed: Vec::new(),
            carried: None,
        });
    }
    wait_for_line(&events, "drag 9,10", &stdout, &stderr);

    let mut lightning = false;
    let mut thunder = false;
    let mut objective = None;
    let mut displayed = None;
    let mut score = None;
    let mut team_added = None;
    let mut boss = None;
    let mut helmet = None;
    let mut slot_item = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let Some((id, body)) = bot.next_play() else {
            continue;
        };
        let mut b = body.as_slice();
        match id {
            AddEntity::ID => {
                let packet = AddEntity::decode(&mut b).unwrap();
                lightning |= packet.entity_type.0 == 78;
            }
            SoundEffect::ID => thunder = true,
            SetObjective::ID => objective = Some(SetObjective::decode(&mut b).unwrap()),
            SetDisplayObjective::ID => {
                displayed = Some(SetDisplayObjective::decode(&mut b).unwrap());
            }
            SetScore::ID => score = Some(SetScore::decode(&mut b).unwrap()),
            PlayerTeam::ID => {
                let packet = PlayerTeam::decode(&mut b).unwrap();
                if matches!(packet.action, TeamAction::Add { .. }) {
                    team_added = Some(packet);
                }
            }
            BossBar::ID => boss = Some(BossBar::decode(&mut b).unwrap()),
            SetEquipment::<ItemStack>::ID => {
                helmet = Some(SetEquipment::<ItemStack>::decode(&mut b).unwrap());
            }
            SetPlayerInventory::<ferrite_item::ItemStack>::ID => {
                let packet = SetPlayerInventory::<ferrite_item::ItemStack>::decode(&mut b).unwrap();
                if packet.slot.0 == 8
                    && packet.item.count > 0
                    && packet
                        .item
                        .components
                        .get(ferrite_item::component_ids::CUSTOM_DATA)
                        .is_some()
                {
                    slot_item = Some(packet);
                }
            }
            _ => {}
        }
        if lightning
            && thunder
            && score.is_some()
            && team_added.is_some()
            && boss.is_some()
            && helmet.is_some()
            && slot_item.is_some()
        {
            break;
        }
    }
    let logs = format!("{}\n{}", stdout.lock().unwrap(), stderr.lock().unwrap());
    assert!(
        lightning,
        "no lightning bolt entity reached the client:\n{logs}"
    );
    assert!(thunder, "no thunder sound reached the client");
    let objective = objective.expect("no objective packet");
    assert_eq!((objective.name.as_str(), objective.mode), ("verify", 0));
    let displayed = displayed.expect("no display slot packet");
    assert_eq!(
        (displayed.slot.0, displayed.objective.as_str()),
        (1, "verify")
    );
    let score = score.expect("no score packet");
    assert_eq!(
        (
            score.entity.as_str(),
            score.objective.as_str(),
            score.value.0
        ),
        ("Bot", "verify", 7)
    );
    let team = team_added.expect("no team packet");
    assert_eq!(team.name, "vteam");
    match team.action {
        TeamAction::Add { params, players } => {
            assert_eq!(
                params.color.map(|c| c.0),
                Some(10),
                "green is team color 10"
            );
            assert_eq!(players, vec!["Bot".to_owned()]);
        }
        _ => unreachable!(),
    }
    match boss.expect("no boss bar packet").action {
        BossBarAction::Add { name, progress, .. } => {
            assert_eq!(name.0, "Boss");
            assert!((progress - 0.5).abs() < f32::EPSILON);
        }
        other => panic!("expected a boss bar add, got {other:?}"),
    }
    let helmet = helmet.expect("no equipment packet");
    assert_eq!(helmet.slots.len(), 1);
    assert_eq!(helmet.slots[0].0, 5, "head slot");
    assert!(helmet.slots[0].1.count > 0, "the helmet was sent empty");

    let slot_item = slot_item.expect("the plugin-set slot 8 never reached the client");
    assert_eq!(slot_item.slot.0, 8);
    assert_eq!(
        slot_item.item.item,
        ferrite_item::item_id("diamond_sword").expect("diamond sword id")
    );
    assert_eq!(slot_item.item.count, 1);
    let data = match slot_item
        .item
        .components
        .get(ferrite_item::component_ids::CUSTOM_DATA)
    {
        Some(ferrite_item::Component::CustomData(data)) => data,
        other => panic!("slot 8 carries no custom_data component: {other:?}"),
    };
    assert_eq!(
        data.get("ferrite:owner"),
        Some(&ferrite_nbt::Tag::String("bot\nline".into()))
    );
    assert_eq!(data.get("ferrite:level"), Some(&ferrite_nbt::Tag::Int(12)));
    assert_eq!(data.get("ferrite:flag"), Some(&ferrite_nbt::Tag::Byte(1)));
    assert_eq!(
        data.get("ferrite:arc"),
        Some(&ferrite_nbt::Tag::IntArray(vec![3, -4]))
    );
    assert_eq!(
        slot_item
            .item
            .components
            .get(ferrite_item::component_ids::TOOLTIP_DISPLAY),
        Some(&ferrite_item::Component::TooltipDisplay(
            ferrite_item::TooltipDisplay {
                hide_tooltip: true,
                hidden_components: vec![
                    ferrite_item::component_ids::ENCHANTMENTS,
                    ferrite_item::component_ids::DYED_COLOR,
                ],
            }
        ))
    );
}

/// Gap 7 end to end: a plugin creates a runtime world from `onEnable`, a client enters and
/// leaves it, the plugin writes a block and spawns an entity there, and after the world is
/// unloaded and reopened the block and the entity are still there.
///
/// Run: `cargo test -p ferrite-server --test plugin_lifecycle runtime_world_gap7 -- --nocapture`
/// (needs Java 25 and a Gradle-built bridge; set `FERRITE_JAVA_PREBUILT=1` to skip Gradle).
#[test]
fn runtime_world_gap7_create_edit_enter_leave_unload_reopen() {
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping gap 7 verification: {error}");
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
    // The test plugin creates its runtime world from onEnable only when this marker exists.
    fs::create_dir(plugins.join("TestPlugin")).unwrap();
    fs::write(plugins.join("TestPlugin/gap7.enabled"), "").unwrap();
    fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();
    let port = free_port();
    fs::write(
        dir.path().join("ferrite.toml"),
        format!(
            "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nview-distance = 2\nmotd = \"Gap 7\"\ngenerator = \"superflat\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
            toml_path(&bridge_jar)
        ),
    )
    .unwrap();
    let mut process = Process(
        server_command()
            .current_dir(dir.path())
            .env("JAVA_HOME", &java_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("starting the server"),
    );
    let (stdout, _stdout_reader) = capture(process.0.stdout.take().unwrap());
    let (stderr, _stderr_reader) = capture(process.0.stderr.take().unwrap());
    let enabled = plugins.join("TestPlugin/enabled.txt");
    let deadline = Instant::now() + Duration::from_secs(180);
    while !enabled.exists() {
        assert!(
            process.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "plugin did not enable:\n{}\n{}",
            stdout.lock().unwrap(),
            stderr.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let events = plugins.join("TestPlugin/events.txt");
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut bot = Bot::join(addr, "Bot", &stdout, &stderr);
    wait_for_line(&events, "join Bot", &stdout, &stderr);

    // Runs `/gap7 <step>` and returns the plugin's `gap7 <step> ...` report. The bot keeps
    // answering the server's packets (teleport confirms, chunk batches) while it waits.
    let respawns = std::cell::Cell::new(0_u32);
    let pump = |bot: &mut Bot| {
        if let Some((id, _)) = bot.next_play()
            && id == Respawn::ID
        {
            respawns.set(respawns.get() + 1);
        }
    };
    let step = |bot: &mut Bot, name: &str| -> String {
        let before = fs::read_to_string(&events)
            .unwrap_or_default()
            .lines()
            .count();
        bot.raw(7, &format!("gap7 {name}"));
        let prefix = format!("gap7 {name}");
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            pump(bot);
            if let Ok(content) = fs::read_to_string(&events)
                && let Some(line) = content
                    .lines()
                    .skip(before)
                    .find(|line| line.starts_with(&prefix))
            {
                return line.to_owned();
            }
            assert!(
                Instant::now() < deadline,
                "no {prefix:?} report:\n{}\n{}\n{}",
                fs::read_to_string(&events).unwrap_or_default(),
                stdout.lock().unwrap(),
                stderr.lock().unwrap()
            );
        }
    };
    // Waits until `/gap7 <step>` reports a line containing `needle`, re-issuing it between
    // tries; the host applies world writes and spawns on the game loop, a little later.
    let settle = |bot: &mut Bot, name: &str, needle: &str| -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let before = fs::read_to_string(&events)
                .unwrap_or_default()
                .lines()
                .count();
            bot.raw(7, &format!("gap7 {name}"));
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                pump(bot);
                let content = fs::read_to_string(&events).unwrap_or_default();
                if let Some(line) = content
                    .lines()
                    .skip(before)
                    .find(|line| line.starts_with(&format!("gap7 {name}")))
                {
                    if line.contains(needle) {
                        return line.to_owned();
                    }
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "gap7 {name} never reported {needle:?}:\n{}\n{}\n{}",
                fs::read_to_string(&events).unwrap_or_default(),
                stdout.lock().unwrap(),
                stderr.lock().unwrap()
            );
            // Each retry is a chat command, and the server's default command limit (a burst of
            // ten, one more per second) kicks "for spamming" a bot that re-issues faster than
            // that refill while a slow host is still applying the write.
            std::thread::sleep(Duration::from_millis(1100));
        }
    };

    // 1. The plugin created the world from onEnable, and finds it again by name.
    assert!(
        fs::read_to_string(&events)
            .unwrap()
            .lines()
            .any(|line| line == "gap7 create true gap7_world"),
        "onEnable did not create the runtime world:\n{}",
        fs::read_to_string(&events).unwrap()
    );
    // 2. It reads a block, writes one, and reads the write back.
    let check = step(&mut bot, "check");
    assert_eq!(check, "gap7 check lookup=true same=true before=AIR");
    // 3. The player enters the world (the client is sent a Respawn to drop the old world's
    // chunks, and another to enter the new one) and the plugin spawns an entity there.
    let enter = step(&mut bot, "enter");
    assert_eq!(enter, "gap7 enter teleport=true");
    let deadline = Instant::now() + Duration::from_secs(30);
    while respawns.get() < 2 && Instant::now() < deadline {
        pump(&mut bot);
    }
    assert!(
        respawns.get() >= 2,
        "entering sent {} Respawn packets",
        respawns.get()
    );
    let spawn = step(&mut bot, "spawn");
    assert!(spawn.starts_with("gap7 spawn "), "{spawn}");
    settle(&mut bot, "entities", "cows=1");
    // The player leaves (the same Respawn pair again).
    let leave = step(&mut bot, "leave");
    assert_eq!(leave, "gap7 leave teleport=true");
    let deadline = Instant::now() + Duration::from_secs(30);
    while respawns.get() < 4 && Instant::now() < deadline {
        pump(&mut bot);
    }
    assert!(
        respawns.get() >= 4,
        "leaving sent {} Respawn packets in total",
        respawns.get()
    );
    // 4. Unload, then reopen: the chunk (its written block and column height) is still there.
    let unload = step(&mut bot, "unload");
    assert_eq!(unload, "gap7 unload true gone=true");
    let reopen = step(&mut bot, "reopen");
    assert!(
        reopen.starts_with("gap7 reopen block=GOLD_BLOCK top=200 "),
        "{reopen}"
    );
    // The world loads its saved entities itself: the cow is back before any player walks in.
    settle(&mut bot, "entities", "cows=1");
    let again = step(&mut bot, "enter");
    assert_eq!(again, "gap7 enter teleport=true");
    settle(&mut bot, "entities", "cows=1");
}
