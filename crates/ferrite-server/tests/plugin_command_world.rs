//! Regression: `createWorld` and `unloadWorld` called straight from a plugin command used to
//! deadlock the game loop, because the command ran on the loop and both calls wait on it.
//!
//! Run: `cargo test -p ferrite-server --test plugin_command_world -- --nocapture`
//! (needs Java 25 and a Gradle-built bridge; set `FERRITE_JAVA_PREBUILT=1` to skip Gradle).
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives in <repo>/crates/<name>")
        .to_owned()
}

fn toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
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

fn build_jars(repo: &Path, java_home: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let project = repo.join("java").join("ferrite-api-impl");
    if std::env::var_os("FERRITE_JAVA_PREBUILT").is_none() {
        let output = Command::new("./gradlew")
            .args([
                "--console=plain",
                ":test-plugin:jar",
                ":test-dependency:jar",
                "shadedJar",
            ])
            .current_dir(&project)
            .env("JAVA_HOME", java_home)
            .output()
            .expect("running Gradle");
        assert!(
            output.status.success(),
            "gradle build failed:\n{}\n{}",
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
    (plugin, dependency, bridge)
}

struct Server {
    process: Process,
    stdin: std::process::ChildStdin,
    plugins: PathBuf,
    _dir: tempfile::TempDir,
}

impl Server {
    /// Starts the server with the test plugin enabled, or `None` when Java 25 is unavailable.
    fn start() -> Option<Self> {
        let java_home = match ferrite_jvm::find_java_home(None) {
            Ok(home) => home.root().to_owned(),
            Err(error) => {
                eprintln!("Java 25 unavailable; skipping: {error}");
                return None;
            }
        };
        let repo = repo_root();
        let (plugin_jar, dependency_jar, bridge_jar) = build_jars(&repo, &java_home);
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        fs::create_dir(&plugins).unwrap();
        fs::copy(plugin_jar, plugins.join("test-plugin.jar")).unwrap();
        fs::copy(dependency_jar, plugins.join("test-dependency.jar")).unwrap();
        fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        fs::write(
            dir.path().join("ferrite.toml"),
            format!(
                "bind-address = \"127.0.0.1:{port}\"\nonline-mode = false\nview-distance = 2\nmotd = \"Cycle\"\ngenerator = \"superflat\"\nplugins-dir = \"plugins\"\nplugin-bridge = \"{}\"\n",
                toml_path(&bridge_jar)
            ),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrite-server"));
        #[cfg(windows)]
        command.arg("--headless");
        let mut process = Process(
            command
                .current_dir(dir.path())
                .env("JAVA_HOME", &java_home)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("starting the server"),
        );
        let enabled = plugins.join("TestPlugin/enabled.txt");
        let deadline = Instant::now() + Duration::from_secs(180);
        while !enabled.exists() {
            assert!(
                process.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
                "plugin did not enable"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let stdin = process.0.stdin.take().unwrap();
        Some(Self {
            process,
            stdin,
            plugins,
            _dir: dir,
        })
    }

    fn events(&self) -> Vec<String> {
        fs::read_to_string(self.plugins.join("TestPlugin/events.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Writes all `lines` in one burst, so the server reads them back-to-back.
    fn console(&mut self, lines: &[&str]) {
        let mut burst = String::new();
        for line in lines {
            burst.push_str(line);
            burst.push('\n');
        }
        self.stdin.write_all(burst.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    /// Waits until `done` accepts the event log; fails fast on a plugin-side cycle error.
    fn wait_events(&mut self, timeout: Duration, what: &str, done: impl Fn(&[String]) -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            let log = self.events();
            if log.iter().any(|line| line.starts_with("cycle error")) {
                panic!("worldcycle failed:\n{}", log.join("\n"));
            }
            if done(&log) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}:\n{}", log.join("\n"));
            assert!(
                self.alive(),
                "the server exited while waiting ({what}):\n{}",
                log.join("\n")
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn alive(&mut self) -> bool {
        self.process.0.try_wait().ok().flatten().is_none()
    }
}

fn cycle_lines(log: &[String]) -> Vec<&str> {
    log.iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("cycle "))
        .collect()
}

#[test]
fn create_and_unload_world_from_a_plugin_command_do_not_hang_the_loop() {
    let Some(mut server) = Server::start() else {
        return;
    };
    server.console(&["worldcycle"]);
    server.wait_events(
        Duration::from_secs(90),
        "createWorld/unloadWorld from a plugin command hung the game loop",
        |log| log.iter().any(|line| line == "cycle unload true gone=true"),
    );
    let log = server.events();
    assert!(
        log.iter().any(|line| line == "cycle create true"),
        "{log:?}"
    );
    // The loop is still alive afterwards: it answers a second command.
    server.console(&["worldcycle"]);
    server.wait_events(
        Duration::from_secs(60),
        "the loop stopped answering after the first cycle",
        |log| {
            log.iter()
                .filter(|line| *line == "cycle unload true gone=true")
                .count()
                >= 2
        },
    );
}

/// Plugin commands run on blocking-pool threads, so back-to-back commands overlap. They must
/// still enter the bridge one at a time: every command's create is followed by its own unload,
/// and nothing hangs, errors or leaves the world behind.
#[test]
fn overlapping_plugin_commands_are_serialized_and_leave_world_state_intact() {
    const COMMANDS: usize = 6;
    let Some(mut server) = Server::start() else {
        return;
    };
    server.console(&["worldcycle"; COMMANDS]);
    server.wait_events(
        Duration::from_secs(120),
        "overlapping plugin commands hung or did not all complete",
        |log| {
            cycle_lines(log)
                .iter()
                .filter(|line| **line == "cycle unload true gone=true")
                .count()
                >= COMMANDS
        },
    );
    let log = server.events();
    let cycles = cycle_lines(&log);
    let expected: Vec<&str> = (0..COMMANDS)
        .flat_map(|_| ["cycle create true", "cycle unload true gone=true"])
        .collect();
    assert_eq!(cycles, expected, "commands interleaved inside the bridge");
    // Still healthy afterwards: a further command completes on the same loop and bridge.
    server.console(&["worldcycle"]);
    server.wait_events(
        Duration::from_secs(60),
        "the loop stopped answering after the overlapping commands",
        |log| {
            cycle_lines(log)
                .iter()
                .filter(|line| **line == "cycle unload true gone=true")
                .count()
                > COMMANDS
        },
    );
    assert!(server.alive(), "the server exited");
}
