use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn stop_on_stdin_shuts_down_server() {
    let dir = std::env::temp_dir().join(format!("ferrite-console-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("ferrite.toml"),
        "bind-address = '127.0.0.1:0'\nonline-mode = false\ngenerator = 'superflat'\n",
    )
    .unwrap();
    // The server refuses to start without `eula=true`; this fixture runs only Ferrite.
    std::fs::write(dir.join("eula.txt"), "eula=true\n").unwrap();
    let mut server = Command::new(env!("CARGO_BIN_EXE_ferrite-server"))
        .arg("--headless")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(server.stdin.take().unwrap(), "/stop").unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = server.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            server.kill().unwrap();
            panic!("server did not stop");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "server exited with {status}");
    std::fs::remove_dir_all(dir).unwrap();
}
