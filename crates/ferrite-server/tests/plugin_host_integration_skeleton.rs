//! Real-host integration skeleton for the Lane 2 (typical) and Lane 3 (NMS) fixtures.
//!
//! This test is ignored until both fixture jars and the host/API jars exist. It deliberately
//! uses the real `PluginHost`, scans both jars from one plugins directory, and drives the same
//! lifecycle a server would: load, join, tick, shutdown. See
//! `docs/handoff/plugin-b-lane-3-nms.md` for the required environment variables.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferrite_plugin_host::{PluginHost, PluginHostConfig};

fn path_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name} to the fixture jar path"))
}

fn name_env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn copy_fixture(source: &Path, plugins: &Path, target: &str) {
    assert!(
        source.is_file(),
        "fixture jar does not exist: {}",
        source.display()
    );
    std::fs::copy(source, plugins.join(target))
        .unwrap_or_else(|error| panic!("copying {}: {error}", source.display()));
}

#[test]
#[ignore = "requires the Lane 2/3 fixture jars, host jar, and API impl jar"]
fn real_host_loads_typical_and_nms_in_one_jvm() {
    let typical = path_env("FERRITE_TYPICAL_PLUGIN_JAR");
    let nms = path_env("FERRITE_NMS_PLUGIN_JAR");
    let host_jar = path_env("FERRITE_HOST_JAR");
    let api_impl = path_env("FERRITE_API_IMPL_JAR");
    let api_impl_class = std::env::var("FERRITE_API_IMPL_CLASS")
        .expect("set FERRITE_API_IMPL_CLASS to the Lane 2 HostApi implementation");

    let plugins = tempfile::tempdir().expect("temporary plugins directory");
    let data = tempfile::tempdir().expect("temporary data directory");
    copy_fixture(&typical, plugins.path(), "typical-plugin.jar");
    copy_fixture(&nms, plugins.path(), "nms-plugin.jar");

    let config = PluginHostConfig {
        enabled: true,
        plugins_dir: plugins.path().to_path_buf(),
        data_dir: data.path().to_path_buf(),
        java_home: std::env::var_os("FERRITE_JAVA_HOME").map(PathBuf::from),
        max_heap_mb: Some(512),
        api_impl_class: Some(api_impl_class),
        host_jar,
        runtime_classpath: vec![api_impl],
        start_timeout: Duration::from_secs(60),
        call_timeout: Duration::from_secs(15),
        ..PluginHostConfig::default()
    };
    let host = PluginHost::start(config).expect("starting the real embedded host");
    let report = host.load_postworld();
    assert!(
        report.failed.is_empty(),
        "fixture load failures: {:?}",
        report.failed
    );

    let loaded: Vec<String> = host
        .loaded()
        .into_iter()
        .map(|plugin| plugin.name)
        .collect();
    let typical_name = name_env("FERRITE_TYPICAL_NAME", "Typical");
    let nms_name = name_env("FERRITE_NMS_NAME", "NMS");
    assert!(
        loaded.iter().any(|name| name == &typical_name),
        "typical fixture {typical_name} was not loaded: {loaded:?}"
    );
    assert!(
        loaded.iter().any(|name| name == &nms_name),
        "NMS fixture {nms_name} was not loaded: {loaded:?}"
    );

    let player = "00000000-0000-0000-0000-000000000001";
    host.fire_event(
        "player_join",
        &serde_json::json!({"player_uuid": player, "player_name": "Alice"}),
    )
    .expect("player_join through the real host");
    host.tick().expect("one real host tick");

    let shutdown = host.shutdown();
    assert!(shutdown.errors.is_empty(), "{:?}", shutdown.errors);
    assert!(shutdown.disabled.iter().any(|name| name == &typical_name));
    assert!(shutdown.disabled.iter().any(|name| name == &nms_name));
}
