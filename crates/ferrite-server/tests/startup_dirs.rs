use std::{
    io::Write,
    path::Path,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

fn run_headless_until_stop(dir: &Path) -> ExitStatus {
    let mut server = Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
        .arg("--headless")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(server.stdin.take().unwrap(), "/stop").unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = server.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            server.kill().unwrap();
            panic!("server did not stop within 90s");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn headless_fresh_install_creates_default_directories_without_plugins() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ferrite.toml"),
        "bind-address = '127.0.0.1:0'\nonline-mode = false\ngenerator = 'superflat'\nstorage = 'anvil'\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();

    let status = run_headless_until_stop(dir.path());
    assert!(status.success(), "server exited with {status}");

    for expected in [
        "config",
        "world",
        "world/data",
        "world/datapacks",
        "world/dimensions/minecraft/overworld/region",
        "world/entities",
        "world/playerdata",
    ] {
        assert!(
            dir.path().join(expected).is_dir(),
            "expected directory {expected} to be created"
        );
    }
    assert!(dir.path().join("world/level.dat").is_file());
    assert!(!dir.path().join("plugins").exists());
    assert!(!dir.path().join("worldgen").exists());
}

#[test]
fn headless_existing_install_fills_missing_dirs_without_overwriting_files() {
    let dir = tempfile::tempdir().unwrap();
    let config_toml = "bind-address = '127.0.0.1:0'\nonline-mode = false\ngenerator = 'superflat'\nstorage = 'anvil'\n";
    let eula_txt = "eula=true\n";
    std::fs::write(dir.path().join("ferrite.toml"), config_toml).unwrap();
    std::fs::write(dir.path().join("eula.txt"), eula_txt).unwrap();

    let playerdata = dir.path().join("world/playerdata");
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&playerdata).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    let player_sentinel = playerdata.join("existing-player.json");
    let config_sentinel = config_dir.join("custom.txt");
    std::fs::write(&player_sentinel, b"keep-player").unwrap();
    std::fs::write(&config_sentinel, b"keep-config").unwrap();

    let status = run_headless_until_stop(dir.path());
    assert!(status.success(), "server exited with {status}");

    for expected in [
        "world/data",
        "world/datapacks",
        "world/dimensions/minecraft/overworld/region",
        "world/entities",
    ] {
        assert!(
            dir.path().join(expected).is_dir(),
            "expected missing directory {expected} to be created"
        );
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ferrite.toml")).unwrap(),
        config_toml
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("eula.txt")).unwrap(),
        eula_txt
    );
    assert_eq!(std::fs::read(&player_sentinel).unwrap(), b"keep-player");
    assert_eq!(std::fs::read(&config_sentinel).unwrap(), b"keep-config");
    assert!(!dir.path().join("plugins").exists());
}

#[test]
fn headless_custom_world_path_creates_only_configured_world_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ferrite.toml"),
        "bind-address = '127.0.0.1:0'\nonline-mode = false\ngenerator = 'superflat'\nstorage = 'anvil'\nworld = 'worlds/custom-survival'\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();

    let status = run_headless_until_stop(dir.path());
    assert!(status.success(), "server exited with {status}");

    for expected in [
        "config",
        "worlds/custom-survival",
        "worlds/custom-survival/data",
        "worlds/custom-survival/datapacks",
        "worlds/custom-survival/dimensions/minecraft/overworld/region",
        "worlds/custom-survival/entities",
        "worlds/custom-survival/playerdata",
    ] {
        assert!(
            dir.path().join(expected).is_dir(),
            "expected directory {expected} to be created"
        );
    }
    assert!(
        dir.path()
            .join("worlds/custom-survival/level.dat")
            .is_file()
    );
    assert!(!dir.path().join("world").exists());
    assert!(!dir.path().join("plugins").exists());
}

#[test]
fn headless_custom_absolute_world_path_creates_external_world_tree() {
    let dir = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let abs_world = external.path().join("abs-survival");
    let toml = format!(
        "bind-address = '127.0.0.1:0'\nonline-mode = false\ngenerator = 'superflat'\nstorage = 'anvil'\nworld = {:?}\n",
        abs_world.to_string_lossy()
    );
    std::fs::write(dir.path().join("ferrite.toml"), toml).unwrap();
    std::fs::write(dir.path().join("eula.txt"), "eula=true\n").unwrap();

    let status = run_headless_until_stop(dir.path());
    assert!(status.success(), "server exited with {status}");

    assert!(dir.path().join("config").is_dir());
    for expected in [
        "data",
        "datapacks",
        "dimensions/minecraft/overworld/region",
        "entities",
        "playerdata",
    ] {
        assert!(
            abs_world.join(expected).is_dir(),
            "expected {expected} under external world"
        );
    }
    assert!(abs_world.join("level.dat").is_file());
    assert!(!dir.path().join("world").exists());
    assert!(!dir.path().join("plugins").exists());
}

#[test]
fn headless_fresh_install_without_eula_creates_config_and_eula_only() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
        .arg("--headless")
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected EULA gate to stop startup"
    );

    assert!(dir.path().join("ferrite.toml").is_file());
    let eula = std::fs::read_to_string(dir.path().join("eula.txt")).unwrap();
    assert!(eula.contains("eula=false"));
    assert!(!dir.path().join("world").exists());
    assert!(!dir.path().join("config").exists());
    assert!(!dir.path().join("worldgen").exists());
    assert!(!dir.path().join("plugins").exists());
}
