//! Compatibility probe: loads every plugin jar in a directory into the embedded JVM bridge and
//! prints the plugin log, so third-party plugins can be tried without a full server.
//!
//! `cargo run -p ferrite-plugin --example load_plugins -- <plugin dir> [PluginName ...]`
//! Needs the prebuilt bridge jar (`gradle shadedJar` in `java/ferrite-api-impl`) and Java 25.

use ferrite_plugin::{PluginConfig, PluginHost, ServerSnapshot};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .expect("usage: load_plugins <plugin dir> [names...]"),
    );
    let names: Vec<String> = args.collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate lives in <repo>/crates/<name>")
        .to_owned();
    let java_home = ferrite_jvm::find_java_home(None)
        .expect("Java 25 is required")
        .root()
        .to_owned();
    let cfg = PluginConfig {
        host_call_handler: Some(std::sync::Arc::new(|call| {
            eprintln!(
                "[host call] {}#{}({})",
                call.owner,
                call.method,
                call.args.join(", ")
            );
            None
        })),
        server_handler: None,
        world_handler: None,
        player_handler: None,
        region_context: None,
        server_snapshot: ServerSnapshot::default(),
        data_dir: root.join("vanilla/26.3/generated"),
        world_view: None,
        world: None,
        worlds: Vec::new(),
        command_handler: None,
        bridge_jar: root.join("java/ferrite-api-impl/build/libs/ferrite-api-impl-all.jar"),
        java_home: Some(java_home),
    };
    let host = PluginHost::start(&dir, cfg).expect("plugin host failed to start");
    for line in host.logs() {
        println!("{line}");
    }
    for name in names {
        println!("== {name}: enabled = {:?}", host.is_plugin_enabled(&name));
    }
}
