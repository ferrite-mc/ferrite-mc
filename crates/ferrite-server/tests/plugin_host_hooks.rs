//! Integration smoke: the real Java host behind `HostHooks`, the adapter the server calls from
//! its event and coordinator-tick paths.
//!
//! The SPI implementation lives in a separate runtime jar, so this also exercises the
//! `host-jar`-first / `runtime-classpath`-next classpath order and `api-impl-class` loading.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use ferrite_plugin_host::{PluginHost, PluginHostConfig};
use ferrite_server::game::plugin_types::{
    BlockPosition, CommandSender, InteractAction, MessageContent, ServerHooks,
};
use ferrite_server::plugin_host::HostHooks;

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

const HOOKS_API: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.CommandMessage;
import dev.ferrite.host.CommandOrigin;
import dev.ferrite.host.CommandResult;
import dev.ferrite.host.EventResult;
import dev.ferrite.host.HostApi;
import dev.ferrite.host.HostContext;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.Map;

public final class HooksApi implements HostApi {
    private Path log;

    private void append(String line) {
        try {
            Files.createDirectories(log.getParent());
            Files.writeString(log, line + "\n", StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }

    @Override
    public void onHostStarted(HostContext context) {
        log = context.dataDir().resolve("hooks-events.txt");
        append("started");
    }

    @Override
    public EventResult fireEvent(String name, Map<String, Object> payload) {
        append("event:" + name + ":" + payload.get("player_name") + ":" + payload.get("action"));
        return name.equals("block_break") ? EventResult.cancel() : EventResult.pass();
    }

    @Override
    public CommandResult runCommand(CommandOrigin origin, String line) {
        append("command:" + line + ":" + origin.playerUuid());
        return CommandResult.handled(origin.playerUuid() == null
                ? CommandMessage.console("ok")
                : CommandMessage.player(origin.playerUuid(), "", "ok"));
    }
}
"#;

const HOOKS_PLUGIN: &str = r#"
package dev.ferrite.host.fixture;

import dev.ferrite.host.JavaPlugin;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardOpenOption;

public final class HooksPlugin extends JavaPlugin {
    private void append(String line) {
        try {
            Files.writeString(pluginDataFolder().resolve("plugin.txt"), line + "\n",
                    StandardOpenOption.CREATE, StandardOpenOption.APPEND);
        } catch (IOException error) {
            throw new RuntimeException(error);
        }
    }

    @Override
    public void onEnable() {
        append("enabled");
        hostContext().schedule(this, () -> append("task"), 0, 0, false);
    }
}
"#;

const PLUGIN_YML: &str = "name: Hooks\nversion: 1.0\nmain: dev.ferrite.host.fixture.HooksPlugin\napi-version: \"1.21\"\nload: STARTUP\ncommands:\n  hook:\n    description: Hooks command\n    usage: /hook\n    aliases: [h]\n    permission: hooks.use\n";

#[test]
fn plugin_host_hooks_round_trip_events_command_scheduler_and_shutdown() {
    let Some(java_home) = ferrite_jvm::find_java_home(None)
        .ok()
        .map(|home| home.root().to_path_buf())
    else {
        eprintln!("SKIP plugin_host_hooks: Java 25+ was not found");
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
    compile(&javac, &host_sources, None, &host_classes);

    let api_src = root
        .path()
        .join("api-src/dev/ferrite/host/fixture/HooksApi.java");
    write(&api_src, HOOKS_API);
    let api_classes = root.path().join("api-classes");
    compile(&javac, &[api_src], Some(&host_classes), &api_classes);

    let plugin_src = root
        .path()
        .join("plugin-src/dev/ferrite/host/fixture/HooksPlugin.java");
    write(&plugin_src, HOOKS_PLUGIN);
    let plugin_classes = root.path().join("plugin-classes");
    compile(&javac, &[plugin_src], Some(&host_classes), &plugin_classes);

    let plugins = root.path().join("plugins");
    let data = root.path().join("data");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let staging = root.path().join("stage-Hooks");
    write(&staging.join("plugin.yml"), PLUGIN_YML);
    let class = "dev/ferrite/host/fixture/HooksPlugin.class";
    let target = staging.join(class);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(plugin_classes.join(class), &target).unwrap();
    build_jar(&jar_tool, &plugins.join("Hooks.jar"), &staging);

    let config = PluginHostConfig {
        enabled: true,
        plugins_dir: plugins,
        data_dir: data.clone(),
        java_home: Some(java_home),
        max_heap_mb: Some(256),
        api_impl_class: Some("dev.ferrite.host.fixture.HooksApi".into()),
        host_jar: host_classes,
        runtime_classpath: vec![api_classes],
        start_timeout: Duration::from_secs(60),
        call_timeout: Duration::from_secs(15),
        ..PluginHostConfig::default()
    };
    let host = Arc::new(PluginHost::start(config).expect("starting the embedded plugin host"));
    assert!(host.is_running());
    let hooks = HostHooks::new(Arc::clone(&host));
    let events = data.join("hooks-events.txt");
    let player = uuid::Uuid::from_u128(7);

    hooks.player_joined(player, "Alice").expect("player_joined");
    assert!(read(&events).contains("event:player_join:Alice:null"));
    hooks.player_left(player, "Alice").expect("player_left");
    assert!(read(&events).contains("event:player_quit:Alice:null"));

    let block = BlockPosition { x: 1, y: 64, z: 2 };
    assert!(
        hooks
            .block_broken(player, "Alice", block, "minecraft:stone")
            .unwrap()
    );
    assert!(
        hooks
            .player_interacted(
                player,
                "Alice",
                InteractAction::RightClickBlock,
                block,
                1,
                "minecraft:stone"
            )
            .unwrap()
    );
    assert!(
        !read(&events).contains("event:block_break"),
        "block events are deferred off the region thread"
    );

    for _ in 0..3 {
        hooks.tick().expect("a server tick");
    }
    let event_log = read(&events);
    assert!(
        event_log.contains("event:block_break:Alice:null"),
        "{event_log}"
    );
    assert!(
        event_log.contains("event:player_interact:Alice:right_click_block"),
        "{event_log}"
    );
    assert!(
        read(&data.join("Hooks/plugin.txt")).contains("task"),
        "the scheduler ran a Java task from the tick"
    );

    let commands = hooks.plugin_commands().expect("plugin commands");
    let command = commands
        .iter()
        .find(|command| command.name == "hook")
        .expect("the plugin.yml command is registered");
    assert_eq!(command.aliases, vec!["h".to_owned()]);
    assert_eq!(command.usage.as_deref(), Some("/hook"));
    let result = hooks
        .run_plugin_command(CommandSender::Player(player), "hook test")
        .expect("run_plugin_command");
    assert!(result.handled);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].player, player);
    assert_eq!(
        result.messages[0].content,
        MessageContent::Plain("ok".into())
    );
    assert!(read(&events).contains(&format!("command:hook test:{player}")));

    let console = hooks
        .run_plugin_command(CommandSender::Console, "console test")
        .expect("console command");
    assert!(console.handled);
    assert_eq!(console.messages.len(), 1);
    assert_eq!(console.messages[0].player, uuid::Uuid::nil());
    assert_eq!(
        console.messages[0].content,
        MessageContent::Plain("ok".into())
    );
    assert!(read(&events).contains("command:console test:null"));

    let report = host.shutdown();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.disabled.contains(&"Hooks".to_owned()));
    assert!(!host.is_running());
}
