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

#[test]
fn create_and_unload_world_from_a_plugin_command_do_not_hang_the_loop() {
    let java_home = match ferrite_jvm::find_java_home(None) {
        Ok(home) => home.root().to_owned(),
        Err(error) => {
            eprintln!("Java 25 unavailable; skipping: {error}");
            return;
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
    // The console runs plugin commands on the game loop, like a player's command does.
    let mut stdin = process.0.stdin.take().unwrap();
    writeln!(stdin, "worldcycle").unwrap();
    stdin.flush().unwrap();
    let events = plugins.join("TestPlugin/events.txt");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let log = fs::read_to_string(&events).unwrap_or_default();
        if log.lines().any(|line| line.starts_with("cycle error")) {
            panic!("worldcycle failed:\n{log}");
        }
        if log
            .lines()
            .any(|line| line == "cycle unload true gone=true")
        {
            assert!(log.lines().any(|line| line == "cycle create true"), "{log}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "createWorld/unloadWorld from a plugin command hung the game loop:\n{log}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // The loop is still alive afterwards: it answers a second command.
    writeln!(stdin, "worldcycle").unwrap();
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while fs::read_to_string(&events)
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == "cycle unload true gone=true")
        .count()
        < 2
    {
        assert!(
            Instant::now() < deadline,
            "the loop stopped answering after the first cycle"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
