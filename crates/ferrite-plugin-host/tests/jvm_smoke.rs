//! End-to-end smoke test: compile `java/ferrite-host`, build fixture plugin jars with the JDK,
//! start the embedded JVM and drive the whole contract v1 lifecycle.
//!
//! This is one test on purpose: JNI allows one JVM per process, so the whole scenario runs in
//! one test binary and one host.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ferrite_plugin_host::{CommandOrigin, HostError, PluginHost, PluginHostConfig};
use serde_json::json;

fn java_tool(home: &Path, name: &str) -> PathBuf {
    home.join("bin")
        .join(format!("{name}{}", if cfg!(windows) { ".exe" } else { "" }))
}

fn run(mut command: Command) -> String {
    let debug = format!("{command:?}");
    let output = command.output().expect("starting a Java tool");
    assert!(
        output.status.success(),
        "command failed: {debug}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn collect_java(directory: &Path, output: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("reading Java sources") {
        let path = entry.expect("reading Java sources").path();
        if path.is_dir() {
            collect_java(&path, output);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "java")
        {
            output.push(path);
        }
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("creating a source directory");
    }
    std::fs::write(path, contents).expect("writing a Java source file");
}

fn compile(javac: &Path, sources: &[PathBuf], classpath: Option<&Path>, output: &Path) {
    std::fs::create_dir_all(output).expect("creating a classes directory");
    let mut command = Command::new(javac);
    command
        .arg("--release")
        .arg("25")
        .arg("-encoding")
        .arg("UTF-8");
    if let Some(classpath) = classpath {
        command.arg("-cp").arg(classpath);
    }
    command.arg("-d").arg(output).args(sources);
    run(command);
}

fn stage(root: &Path, name: &str, plugin_yml: &str, classes: &[(&str, &Path)]) -> PathBuf {
    let staging = root.join(format!("stage-{name}"));
    write(&staging.join("plugin.yml"), plugin_yml);
    for (relative, source) in classes {
        let target = staging.join(relative);
        std::fs::create_dir_all(target.parent().expect("class file parent")).unwrap();
        std::fs::copy(source, &target).expect("copying a class file into the plugin staging dir");
    }
    staging
}

fn build_jar(jar: &Path, output: &Path, staging: &Path) {
    std::fs::create_dir_all(output.parent().expect("jar parent")).unwrap();
    let mut command = Command::new(jar);
    command
        .arg("--create")
        .arg("--file")
        .arg(output)
        .arg("-C")
        .arg(staging)
        .arg(".");
    run(command);
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

fn count(path: &Path, line: &str) -> usize {
    read(path).lines().filter(|entry| *entry == line).count()
}

const TEST_API: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.CommandMessage;
import dev.ferrite.host.CommandOrigin;
import dev.ferrite.host.CommandResult;
import dev.ferrite.host.EventResult;
import dev.ferrite.host.HostApi;
import dev.ferrite.host.HostContext;
import dev.ferrite.host.HostPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.Map;

public final class TestApi implements HostApi {
    private Path events;

    private void append(String line) {
        try {
            Files.createDirectories(events.getParent());
            Files.writeString(events, line + "\n", StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }

    @Override
    public void onHostStarted(HostContext context) {
        events = context.dataDir().resolve("api-events.txt");
        append("started:" + context.apiVersion());
    }

    @Override
    public void onPluginEnabled(HostPlugin plugin) {
        append("enabled:" + plugin.pluginName());
    }

    @Override
    public void onPluginDisabled(HostPlugin plugin) {
        append("disabled:" + plugin.pluginName());
    }

    @Override
    public EventResult fireEvent(String name, Map<String, Object> payload) {
        append("event:" + name + ":" + payload.get("player_name"));
        return name.equals("block_break") ? EventResult.cancel() : EventResult.pass();
    }

    @Override
    public CommandResult runCommand(CommandOrigin origin, String line) {
        append("command:" + line);
        return CommandResult.handled(CommandMessage.console("ok"));
    }

    @Override
    public void onHostStopping() {
        append("stopping");
    }
}
"#;

const GOOD_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.HostBridge;
import dev.ferrite.host.JavaPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;

public final class GoodPlugin extends JavaPlugin {
    private void append(String line) {
        try {
            Files.writeString(pluginDataFolder().resolve("lifecycle.txt"), line + "\n",
                    StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }

    @Override
    public void onLoad() {
        append("onLoad");
    }

    @Override
    public void onEnable() {
        append("onEnable:" + HostBridge.call("host.ping", "{}"));
        hostContext().schedule(this, () -> append("task"), 1, 0, false);
        hostContext().schedule(this, () -> append("repeat"), 1, 2, false);
    }

    @Override
    public void onDisable() {
        append("onDisable");
    }
}
"#;

const STARTUP_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.JavaPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;

public final class StartupPlugin extends JavaPlugin {
    @Override
    public void onEnable() {
        try {
            Files.writeString(pluginDataFolder().resolve("lifecycle.txt"), "startup\n",
                    StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }
}
"#;

const POSTWORLD_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.JavaPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;

public final class PostworldPlugin extends JavaPlugin {
    @Override
    public void onEnable() {
        try {
            Files.writeString(pluginDataFolder().resolve("lifecycle.txt"), "postworld\n",
                    StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }
}
"#;

const BROKEN_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.JavaPlugin;

public final class BrokenPlugin extends JavaPlugin {
    @Override
    public void onEnable() {
        throw new IllegalStateException("boom");
    }
}
"#;

const ISOLATED_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.JavaPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;

public final class IsolatedPlugin extends JavaPlugin {
    @Override
    public void onEnable() {
        try {
            Class<?> helper = Class.forName("com.example.shared.Helper", true, getClass().getClassLoader());
            Object value = helper.getMethod("value").invoke(null);
            Files.writeString(pluginDataFolder().resolve("helper.txt"), String.valueOf(value));
        } catch (ReflectiveOperationException | IOException error) {
            throw new RuntimeException(error);
        }
    }
}
"#;

fn helper_source(value: &str) -> String {
    format!(
        "package com.example.shared;\npublic final class Helper {{\n    public static String value() {{ return \"{value}\"; }}\n}}\n"
    )
}

#[test]
fn embedded_jvm_lifecycle_rpc_scheduler_isolation_and_shutdown() {
    let Some(java_home) = ferrite_jvm::find_java_home(None)
        .ok()
        .map(|home| home.root().to_path_buf())
    else {
        eprintln!("SKIP jvm_smoke: Java 25+ was not found");
        return;
    };
    let javac = java_tool(&java_home, "javac");
    let jar_tool = java_tool(&java_home, "jar");

    let root = tempfile::tempdir().expect("temporary test directory");
    let host_src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../java/ferrite-host/src/main/java");
    let host_classes = root.path().join("host-classes");
    let mut host_sources = Vec::new();
    collect_java(&host_src, &mut host_sources);
    let test_api = root
        .path()
        .join("test-src/dev/ferrite/host/fixture/TestApi.java");
    write(&test_api, TEST_API);
    host_sources.push(test_api);
    compile(&javac, &host_sources, None, &host_classes);

    let fixture_src = root.path().join("fixture-src");
    write(
        &fixture_src.join("dev/ferrite/host/fixture/GoodPlugin.java"),
        GOOD_PLUGIN,
    );
    write(
        &fixture_src.join("dev/ferrite/host/fixture/StartupPlugin.java"),
        STARTUP_PLUGIN,
    );
    write(
        &fixture_src.join("dev/ferrite/host/fixture/PostworldPlugin.java"),
        POSTWORLD_PLUGIN,
    );
    write(
        &fixture_src.join("dev/ferrite/host/fixture/BrokenPlugin.java"),
        BROKEN_PLUGIN,
    );
    let mut fixture_sources = Vec::new();
    collect_java(&fixture_src, &mut fixture_sources);
    let fixture_classes = root.path().join("fixture-classes");
    compile(
        &javac,
        &fixture_sources,
        Some(&host_classes),
        &fixture_classes,
    );

    let isolation_one = root.path().join("isolation-one");
    write(
        &isolation_one.join("dev/ferrite/host/fixture/IsolatedPlugin.java"),
        ISOLATED_PLUGIN,
    );
    write(
        &isolation_one.join("com/example/shared/Helper.java"),
        &helper_source("one"),
    );
    let mut isolation_one_sources = Vec::new();
    collect_java(&isolation_one, &mut isolation_one_sources);
    let isolation_one_classes = root.path().join("isolation-one-classes");
    compile(
        &javac,
        &isolation_one_sources,
        Some(&host_classes),
        &isolation_one_classes,
    );

    let isolation_two = root.path().join("isolation-two");
    write(
        &isolation_two.join("dev/ferrite/host/fixture/IsolatedPlugin.java"),
        ISOLATED_PLUGIN,
    );
    write(
        &isolation_two.join("com/example/shared/Helper.java"),
        &helper_source("two"),
    );
    let mut isolation_two_sources = Vec::new();
    collect_java(&isolation_two, &mut isolation_two_sources);
    let isolation_two_classes = root.path().join("isolation-two-classes");
    compile(
        &javac,
        &isolation_two_sources,
        Some(&host_classes),
        &isolation_two_classes,
    );

    let plugins = root.path().join("plugins");
    let data = root.path().join("data");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::create_dir_all(&data).unwrap();

    let good_yml = "name: Good\nversion: 1.0\nmain: dev.ferrite.host.fixture.GoodPlugin\napi-version: \"1.21\"\ncommands:\n  good:\n    description: Good command\n    usage: /good\n    aliases: [g]\n    permission: good.use\n";
    let startup_yml = "name: Startup\nversion: 1.0\nmain: dev.ferrite.host.fixture.StartupPlugin\napi-version: \"1.21\"\nload: STARTUP\n";
    let postworld_yml = "name: Postworld\nversion: 1.0\nmain: dev.ferrite.host.fixture.PostworldPlugin\napi-version: \"1.21\"\ndepend: [Good]\n";
    let broken_yml = "name: Broken\nversion: 1.0\nmain: dev.ferrite.host.fixture.BrokenPlugin\napi-version: \"1.21\"\n";
    let isolated_one_yml = "name: IsolatedOne\nversion: 1.0\nmain: dev.ferrite.host.fixture.IsolatedPlugin\napi-version: \"1.21\"\n";
    let isolated_two_yml = "name: IsolatedTwo\nversion: 1.0\nmain: dev.ferrite.host.fixture.IsolatedPlugin\napi-version: \"1.21\"\n";

    let stage_good = stage(
        &plugins,
        "Good",
        good_yml,
        &[(
            "dev/ferrite/host/fixture/GoodPlugin.class",
            &fixture_classes.join("dev/ferrite/host/fixture/GoodPlugin.class"),
        )],
    );
    build_jar(&jar_tool, &plugins.join("Good.jar"), &stage_good);

    let stage_startup = stage(
        &plugins,
        "Startup",
        startup_yml,
        &[(
            "dev/ferrite/host/fixture/StartupPlugin.class",
            &fixture_classes.join("dev/ferrite/host/fixture/StartupPlugin.class"),
        )],
    );
    build_jar(&jar_tool, &plugins.join("Startup.jar"), &stage_startup);

    let stage_postworld = stage(
        &plugins,
        "Postworld",
        postworld_yml,
        &[(
            "dev/ferrite/host/fixture/PostworldPlugin.class",
            &fixture_classes.join("dev/ferrite/host/fixture/PostworldPlugin.class"),
        )],
    );
    build_jar(&jar_tool, &plugins.join("Postworld.jar"), &stage_postworld);

    let stage_broken = stage(
        &plugins,
        "Broken",
        broken_yml,
        &[(
            "dev/ferrite/host/fixture/BrokenPlugin.class",
            &fixture_classes.join("dev/ferrite/host/fixture/BrokenPlugin.class"),
        )],
    );
    build_jar(&jar_tool, &plugins.join("Broken.jar"), &stage_broken);

    let stage_isolated_one = stage(
        &plugins,
        "IsolatedOne",
        isolated_one_yml,
        &[
            (
                "dev/ferrite/host/fixture/IsolatedPlugin.class",
                &isolation_one_classes.join("dev/ferrite/host/fixture/IsolatedPlugin.class"),
            ),
            (
                "com/example/shared/Helper.class",
                &isolation_one_classes.join("com/example/shared/Helper.class"),
            ),
        ],
    );
    build_jar(
        &jar_tool,
        &plugins.join("IsolatedOne.jar"),
        &stage_isolated_one,
    );

    let stage_isolated_two = stage(
        &plugins,
        "IsolatedTwo",
        isolated_two_yml,
        &[
            (
                "dev/ferrite/host/fixture/IsolatedPlugin.class",
                &isolation_two_classes.join("dev/ferrite/host/fixture/IsolatedPlugin.class"),
            ),
            (
                "com/example/shared/Helper.class",
                &isolation_two_classes.join("com/example/shared/Helper.class"),
            ),
        ],
    );
    build_jar(
        &jar_tool,
        &plugins.join("IsolatedTwo.jar"),
        &stage_isolated_two,
    );

    let config = PluginHostConfig {
        enabled: true,
        plugins_dir: plugins.clone(),
        data_dir: data.clone(),
        java_home: Some(java_home),
        max_heap_mb: Some(256),
        api_impl_class: Some("dev.ferrite.host.fixture.TestApi".into()),
        host_jar: host_classes.clone(),
        start_timeout: Duration::from_secs(60),
        call_timeout: Duration::from_secs(15),
        ..PluginHostConfig::default()
    };
    let second_start = config.clone();
    let host = PluginHost::start(config).expect("starting the embedded plugin host");

    assert!(host.is_running());
    assert!(!host.is_dormant());
    assert!(
        data.join("Startup/lifecycle.txt").exists(),
        "STARTUP plugins load before the world"
    );
    assert!(
        !data.join("Postworld/lifecycle.txt").exists(),
        "POSTWORLD plugins wait for load_postworld"
    );
    assert!(
        !data.join("Good/lifecycle.txt").exists(),
        "Good is a POSTWORLD plugin"
    );

    let report = host.load_postworld();
    assert!(report.failed.iter().all(|failure| {
        failure.plugin.as_deref() != Some("Good")
            && failure.plugin.as_deref() != Some("Postworld")
            && failure.plugin.as_deref() != Some("IsolatedOne")
            && failure.plugin.as_deref() != Some("IsolatedTwo")
    }));
    let broken = report
        .failed
        .iter()
        .find(|failure| failure.plugin.as_deref() == Some("Broken"))
        .expect("the broken plugin is reported");
    assert_eq!(broken.code, "plugin_failed");
    assert!(broken.error.contains("boom"), "{}", broken.error);
    assert!(
        report.loaded.iter().any(|plugin| plugin.name == "Good"),
        "other plugins still load: {:?}",
        report.loaded
    );
    assert!(
        data.join("Postworld/lifecycle.txt").exists(),
        "POSTWORLD plugins load after load_postworld"
    );

    let good = read(&data.join("Good/lifecycle.txt"));
    assert!(good.contains("onLoad"), "{good}");
    assert!(good.contains("onEnable:"), "{good}");
    assert!(
        good.contains("pong"),
        "the native RPC round trip works: {good}"
    );
    assert_eq!(read(&data.join("IsolatedOne/helper.txt")), "one");
    assert_eq!(read(&data.join("IsolatedTwo/helper.txt")), "two");

    let events = data.join("api-events.txt");
    let event_log = read(&events);
    assert!(event_log.contains("started:26.3"), "{event_log}");
    assert!(event_log.contains("enabled:Good"), "{event_log}");
    assert!(event_log.contains("enabled:Postworld"), "{event_log}");

    let join = host
        .fire_event(
            "player_join",
            &json!({"player_uuid": "00000000-0000-0000-0000-000000000001", "player_name": "Alice"}),
        )
        .expect("player_join");
    assert!(!join.cancelled);
    let block = host
        .fire_event(
            "block_break",
            &json!({
                "player_uuid": "00000000-0000-0000-0000-000000000001",
                "player_name": "Alice",
                "world": "minecraft:overworld",
                "x": 1, "y": 64, "z": 2,
                "material": "minecraft:stone"
            }),
        )
        .expect("block_break");
    assert!(block.cancelled, "the test API cancels block_break");
    let unknown = host.fire_event("server_start", &json!({}));
    assert!(matches!(unknown, Err(HostError::UnknownEvent(_))));

    for _ in 0..4 {
        host.tick().expect("a tick");
    }
    assert_eq!(count(&data.join("Good/lifecycle.txt"), "task"), 1);
    assert_eq!(count(&data.join("Good/lifecycle.txt"), "repeat"), 2);

    let command = host
        .run_command(CommandOrigin::Console, "good")
        .expect("run_command");
    assert!(command.handled);
    assert_eq!(command.messages.len(), 1);
    assert_eq!(command.messages[0].content, "ok");
    assert!(
        host.commands().iter().any(|command| command.name == "good"),
        "plugin.yml commands are exposed: {:?}",
        host.commands()
    );

    assert!(matches!(
        PluginHost::start(second_start),
        Err(HostError::AlreadyStarted)
    ));

    let shutdown = host.shutdown();
    assert!(shutdown.errors.is_empty(), "{:?}", shutdown.errors);
    assert!(shutdown.disabled.contains(&"Good".to_owned()));
    assert!(shutdown.disabled.contains(&"Startup".to_owned()));
    assert!(shutdown.disabled.contains(&"Postworld".to_owned()));
    assert!(!shutdown.disabled.contains(&"Broken".to_owned()));
    assert!(read(&data.join("Good/lifecycle.txt")).contains("onDisable"));
    assert!(read(&events).contains("stopping"));
    assert!(!host.is_running());
    assert!(matches!(
        host.fire_event("player_join", &json!({})),
        Err(HostError::NotRunning)
    ));
    assert_eq!(
        host.status()
            .get("running")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
}
