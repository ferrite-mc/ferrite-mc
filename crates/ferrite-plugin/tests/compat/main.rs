//! Opt-in real-plugin compatibility probe; each fixture runs in a fresh process and directory.
use ferrite_plugin::{CommandSender, PluginConfig, PluginHost};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

/// Generated vanilla data root, resolved like the server binary: `FERRITE_VANILLA_ROOT`
/// wins, otherwise `<crate>/../../vanilla/26.3/generated`.
fn resolve_vanilla_data(manifest_dir: &Path, override_root: Option<&OsStr>) -> PathBuf {
    override_root.map_or_else(
        || manifest_dir.join("../../vanilla/26.3/generated"),
        PathBuf::from,
    )
}

fn vanilla_data_dir() -> PathBuf {
    resolve_vanilla_data(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        std::env::var_os("FERRITE_VANILLA_ROOT").as_deref(),
    )
}

fn probe_config() -> PluginConfig {
    PluginConfig {
        bridge_jar: root().join("java/ferrite-api-impl/build/libs/ferrite-api-impl-all.jar"),
        data_dir: vanilla_data_dir(),
        ..Default::default()
    }
}

#[test]
fn compat_probe() {
    let Some(jar) = std::env::var_os("FERRITE_COMPAT_PROBE") else {
        return;
    };
    let jar = PathBuf::from(jar);
    let plugins = tempfile::tempdir().unwrap();
    let target = plugins.path().join("fixture.jar");
    fs::copy(&jar, &target).unwrap();
    let meta = match ferrite_plugin_meta::read_plugin_meta(&target) {
        Ok(meta) => meta,
        Err(e) => {
            println!("FERRITE_RESULT metadata error: {e}");
            return;
        }
    };
    let config = probe_config();
    match PluginHost::start(plugins.path(), config) {
        Ok(host) => {
            println!(
                "FERRITE_RESULT enabled={:?}",
                host.is_plugin_enabled(&meta.name)
            );
            let command = match meta.name.as_str() {
                "Vault" => Some("vault-info"),
                "LuckPerms" => Some("luckperms"),
                "PlaceholderAPI" => Some("placeholderapi version"),
                "Essentials" => Some("help"),
                _ => None,
            };
            if let Some(command) = command {
                println!(
                    "FERRITE_COMMAND {command}: {:?}",
                    host.dispatch_command(CommandSender::Console, command)
                );
                if let Err(error) = host.tick() {
                    println!("FERRITE_TICK {error}");
                }
            }
            for line in host.logs() {
                println!("FERRITE_LOG {line}");
            }
            if let Err(error) = host.shutdown() {
                println!("FERRITE_SHUTDOWN {error}");
            }
        }
        Err(error) => println!("FERRITE_RESULT failed: {error}"),
    }
}

#[test]
fn compatibility_report() {
    let root = root();
    let fixtures = root.join("plugins-test");
    // Keep the checked-in report when no fixture is available to probe.
    if !["Vault", "LuckPerms", "EssentialsX", "PlaceholderAPI"]
        .iter()
        .any(|name| fixtures.join(format!("{name}.jar")).exists())
    {
        return;
    }
    let mut report = String::from(
        "# Plugin compatibility report\n\nEach plugin is tested alone in an isolated process with a basic console command; optional integrations are not exercised.\n\n",
    );
    for name in ["Vault", "LuckPerms", "EssentialsX", "PlaceholderAPI"] {
        let jar = fixtures.join(format!("{name}.jar"));
        report.push_str(&format!("## {name}\n\n"));
        if !jar.exists() {
            report.push_str("Fixture absent; run `python tools/fetch_test_plugins.py`.\n\n");
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("output.log");
        let out = fs::File::create(&log).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "compat_probe", "--nocapture"])
            .env("FERRITE_COMPAT_PROBE", &jar)
            .current_dir(dir.path())
            .stdout(Stdio::from(out.try_clone().unwrap()))
            .stderr(Stdio::from(out))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break format!("{status}");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                break "timed out after 60 seconds".to_owned();
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let output = String::from_utf8_lossy(&fs::read(log).unwrap()).into_owned();
        assert!(
            output.contains("FERRITE_RESULT") || status.contains("timed out"),
            "fixture {name} produced no result: {status}"
        );
        report.push_str(&format!(
            "Process: {status}\n\n```text\n{}\n```\n\n",
            output.replace("```", "'''")
        ));
    }
    fs::write(root.join("compat-report.md"), report).unwrap();
}

/// The probe used to inherit `PluginConfig::default()`, leaving `data_dir` empty and skipping
/// `Bootstrap.configureDataDir`, unlike the production server.
#[test]
fn probe_configures_generated_vanilla_data() {
    let config = probe_config();
    assert!(
        !config.data_dir.as_os_str().is_empty(),
        "the compat probe must not leave PluginConfig::data_dir unset"
    );
    assert_eq!(
        config.data_dir,
        resolve_vanilla_data(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            std::env::var_os("FERRITE_VANILLA_ROOT").as_deref(),
        )
    );
    assert_eq!(
        resolve_vanilla_data(Path::new("/repo"), Some(OsStr::new("/generated"))),
        PathBuf::from("/generated")
    );
    assert_eq!(
        resolve_vanilla_data(Path::new("/repo"), None),
        Path::new("/repo").join("../../vanilla/26.3/generated")
    );
}
