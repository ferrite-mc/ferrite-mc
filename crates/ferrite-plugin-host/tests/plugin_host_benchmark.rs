//! Repeatable embedded-JVM plugin-host benchmark harness.
//!
//! Starts the real embedded JVM with `typical-plugin` and `nms-plugin` via `FerriteHostApi`
//! and measures:
//! - plugin discovery, JVM/host start, `onLoad`/`onEnable`, and `onDisable` latency,
//! - process RSS before start, after plugin load, and after workload,
//! - event dispatch throughput (`player_join` and repeated `block_break`),
//! - command dispatch throughput (`ferrite-hello`),
//! - scheduler task execution (`PluginHost::tick` driving Java + host tasks).
//!
//! Run explicitly with:
//! `cargo test -p ferrite-plugin-host --release --test plugin_host_benchmark -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ferrite_plugin_host::{
    BlockGetRequest, BlockSetAccepted, BlockSetRequest, BlockSnapshot, CommandOrigin,
    HostStateProvider, InventorySetAccepted, PlayerInventorySetRequest, PlayerInventorySnapshot,
    PluginHost, PluginHostConfig, StateError,
};
use serde_json::json;

const WARMUP_ITERS: usize = 50;
const JOIN_ITERS: usize = 200;
const EVENT_ITERS: usize = 500;
const COMMAND_ITERS: usize = 500;
const TICK_ITERS: usize = 500;

struct BenchStateProvider {
    next_request: AtomicU64,
}

impl BenchStateProvider {
    fn new() -> Self {
        Self {
            next_request: AtomicU64::new(0),
        }
    }
}

impl HostStateProvider for BenchStateProvider {
    fn block_get(&self, request: &BlockGetRequest) -> Result<BlockSnapshot, StateError> {
        Ok(BlockSnapshot {
            world: request.world.clone(),
            x: request.x,
            y: request.y,
            z: request.z,
            state_id: 1,
            material: "minecraft:stone".to_owned(),
            properties: BTreeMap::new(),
            loaded: true,
        })
    }

    fn block_set_async(&self, _request: &BlockSetRequest) -> Result<BlockSetAccepted, StateError> {
        Ok(BlockSetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::Relaxed) + 1,
        })
    }

    fn chunk_loaded(&self, _world: &str, _x: i32, _z: i32) -> Result<bool, StateError> {
        Ok(true)
    }

    fn player_inventory_get(&self, uuid: &str) -> Result<PlayerInventorySnapshot, StateError> {
        Ok(PlayerInventorySnapshot {
            uuid: uuid.to_owned(),
            held_slot: 0,
            slots: BTreeMap::new(),
        })
    }

    fn player_inventory_set_async(
        &self,
        _request: &PlayerInventorySetRequest,
    ) -> Result<InventorySetAccepted, StateError> {
        Ok(InventorySetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::Relaxed) + 1,
        })
    }

    fn supported_ops(&self) -> Vec<&'static str> {
        vec![
            "world.block_get",
            "world.block_set_async",
            "world.chunk_loaded",
            "player.inventory_get",
            "player.inventory_set_async",
        ]
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn find_jar(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(prefix)
            && name.ends_with(".jar")
            && !name.contains("-sources")
            && !name.contains("-javadoc")
            && !name.contains("-paper")
        {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                best = Some((modified, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn ensure_java_artifacts(root: &Path) -> (PathBuf, Vec<PathBuf>, PathBuf, PathBuf) {
    let java_dir = root.join("java");
    let classpath_file = java_dir.join("ferrite-api-impl/build/paper-api-classpath.txt");

    let need_build = find_jar(&java_dir.join("ferrite-host/build/libs"), "ferrite-host").is_none()
        || find_jar(
            &java_dir.join("ferrite-api-impl/build/libs"),
            "ferrite-api-impl",
        )
        .is_none()
        || find_jar(&java_dir.join("ferrite-nms/build/libs"), "ferrite-nms").is_none()
        || find_jar(
            &java_dir.join("fixtures/typical-plugin/build/libs"),
            "typical-plugin",
        )
        .is_none()
        || find_jar(
            &java_dir.join("fixtures/nms-plugin/build/libs"),
            "nms-plugin",
        )
        .is_none()
        || !classpath_file.is_file();

    if need_build {
        let gradlew = if cfg!(windows) {
            java_dir.join("gradlew.bat")
        } else {
            java_dir.join("gradlew")
        };
        assert!(
            gradlew.is_file(),
            "missing Gradle wrapper at {}; build Java artifacts before running benchmark",
            gradlew.display()
        );
        let status = Command::new(&gradlew)
            .args([
                ":ferrite-host:jar",
                ":ferrite-api-impl:jar",
                ":ferrite-api-impl:printApiClasspath",
                ":ferrite-nms:jar",
                ":fixtures:typical-plugin:jar",
                ":fixtures:nms-plugin:jar",
                "--no-daemon",
            ])
            .current_dir(&java_dir)
            .status()
            .expect("running gradlew to build Plugin B jars");
        assert!(status.success(), "gradlew failed with {status}");
    }

    let host_jar =
        find_jar(&java_dir.join("ferrite-host/build/libs"), "ferrite-host").expect("host jar");
    let api_impl_jar = find_jar(
        &java_dir.join("ferrite-api-impl/build/libs"),
        "ferrite-api-impl",
    )
    .expect("api-impl jar");
    let nms_jar =
        find_jar(&java_dir.join("ferrite-nms/build/libs"), "ferrite-nms").expect("nms core jar");
    let typical_jar = find_jar(
        &java_dir.join("fixtures/typical-plugin/build/libs"),
        "typical-plugin",
    )
    .expect("typical-plugin jar");
    let nms_plugin_jar = find_jar(
        &java_dir.join("fixtures/nms-plugin/build/libs"),
        "nms-plugin",
    )
    .expect("nms-plugin jar");

    let sep = if cfg!(windows) { ';' } else { ':' };
    let api_deps = std::fs::read_to_string(&classpath_file)
        .expect("reading paper-api-classpath.txt")
        .split(sep)
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();

    // Keep ferrite-api-impl ahead of paper-api so shaded/shadowed classes resolve first.
    let mut runtime_classpath = vec![api_impl_jar, nms_jar];
    runtime_classpath.extend(api_deps);

    (host_jar, runtime_classpath, typical_jar, nms_plugin_jar)
}

/// Reads current and peak resident set size (in KiB) from `/proc/self/status` on Linux.
fn read_proc_rss_kib() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut vm_rss = None;
    let mut vm_hwm = None;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            vm_rss = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("VmHWM:") {
            vm_hwm = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        }
    }
    Some((vm_rss?, vm_hwm.unwrap_or(vm_rss?)))
}

struct LatencyStats {
    iters: usize,
    total: Duration,
    median: Duration,
    p95: Duration,
    min: Duration,
    max: Duration,
    throughput_per_sec: f64,
}

fn summarize(samples: &mut [Duration], total: Duration) -> LatencyStats {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    let n = samples.len();
    let median = if n.is_multiple_of(2) {
        (samples[n / 2 - 1] + samples[n / 2]) / 2
    } else {
        samples[n / 2]
    };
    let p95_idx = ((n as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(n - 1);
    let p95 = samples[p95_idx];
    let throughput_per_sec = n as f64 / total.as_secs_f64();
    LatencyStats {
        iters: n,
        total,
        median,
        p95,
        min: samples[0],
        max: samples[n - 1],
        throughput_per_sec,
    }
}

fn print_stats(label: &str, stats: &LatencyStats) {
    println!(
        "{label:<28} iters={:<5} total={:>8.2} ms  median={:>8.2} us  p95={:>8.2} us  min={:>7.2} us  max={:>8.2} us  throughput={:>9.1} ops/s",
        stats.iters,
        stats.total.as_secs_f64() * 1_000.0,
        stats.median.as_secs_f64() * 1_000_000.0,
        stats.p95.as_secs_f64() * 1_000_000.0,
        stats.min.as_secs_f64() * 1_000_000.0,
        stats.max.as_secs_f64() * 1_000_000.0,
        stats.throughput_per_sec,
    );
}

#[test]
#[ignore = "runs the embedded JVM plugin-host benchmark; invoke with --release -- --ignored --nocapture"]
fn benchmark_embedded_jvm_plugin_host() {
    let root = workspace_root();
    let (host_jar, runtime_classpath, typical_jar, nms_plugin_jar) = ensure_java_artifacts(&root);

    let temp = tempfile::tempdir().expect("creating benchmark temp dir");
    let plugins_dir = temp.path().join("plugins");
    let data_dir = temp.path().join("plugin-data");
    std::fs::create_dir_all(&plugins_dir).expect("creating plugins dir");
    std::fs::create_dir_all(&data_dir).expect("creating data dir");
    std::fs::copy(&typical_jar, plugins_dir.join("typical-plugin.jar"))
        .expect("copying typical-plugin.jar");
    std::fs::copy(&nms_plugin_jar, plugins_dir.join("nms-plugin.jar"))
        .expect("copying nms-plugin.jar");

    let rss_before = read_proc_rss_kib();

    // 1. Plugin discovery latency (ferrite-plugin-meta scan).
    let discovery_start = Instant::now();
    let plan = ferrite_plugin_meta::scan_directory(&plugins_dir).expect("scanning plugins dir");
    let discovery_elapsed = discovery_start.elapsed();
    assert_eq!(
        plan.startup.len() + plan.postworld.len(),
        2,
        "expected 2 discovered plugins"
    );
    assert!(plan.failed.is_empty(), "unexpected failed plugins");

    // 2. Host + embedded JVM start latency.
    let vanilla_generated = std::env::var_os("FERRITE_VANILLA_GENERATED")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("vanilla").join("26.3").join("generated"));
    assert!(
        vanilla_generated.is_dir(),
        "missing vanilla generated data at {}; run `python3 tools/fetch_vanilla.py` or set FERRITE_VANILLA_GENERATED",
        vanilla_generated.display()
    );
    let jvm_args = vec![format!(
        "-Dferrite.vanilla.generated={}",
        vanilla_generated.display()
    )];

    let config = PluginHostConfig {
        enabled: true,
        plugins_dir: plugins_dir.clone(),
        data_dir: data_dir.clone(),
        java_home: None,
        max_heap_mb: Some(512),
        jvm_args,
        api_version: "26.3".to_owned(),
        host_jar,
        runtime_classpath,
        api_impl_class: Some("dev.ferrite.api.FerriteHostApi".to_owned()),
        start_timeout: Duration::from_secs(60),
        call_timeout: Duration::from_secs(15),
        ..PluginHostConfig::default()
    };

    let start_begin = Instant::now();
    let host = PluginHost::start(config).expect("starting PluginHost with embedded JVM");
    let host_start_elapsed = start_begin.elapsed();
    assert!(host.is_running(), "host must be running");

    host.install_state_provider(Arc::new(BenchStateProvider::new()))
        .expect("installing state provider");

    // 3. Postworld plugin load + onLoad + onEnable latency.
    let load_begin = Instant::now();
    let load_report = host.load_postworld();
    let load_enable_elapsed = load_begin.elapsed();
    assert!(
        load_report.failed.is_empty(),
        "expected no plugin load failures, got: {:?}",
        load_report.failed
    );
    assert_eq!(
        load_report.loaded.len(),
        2,
        "expected typical-plugin and nms-plugin to enable"
    );

    let rss_after_load = read_proc_rss_kib();

    // 4. Event dispatch throughput: player_join (paired with player_quit to keep online state clean)
    //    plus repeated block_break.
    let join_payload = json!({
        "player_uuid": "00000000-0000-3000-8000-000000000001",
        "player_name": "BenchPlayer"
    });
    let quit_payload = json!({
        "player_uuid": "00000000-0000-3000-8000-000000000001",
        "player_name": "BenchPlayer"
    });
    for _ in 0..WARMUP_ITERS {
        host.fire_event("player_join", &join_payload)
            .expect("warmup player_join");
        host.fire_event("player_quit", &quit_payload)
            .expect("warmup player_quit");
    }
    let mut join_samples = Vec::with_capacity(JOIN_ITERS);
    let mut join_elapsed_sum = Duration::ZERO;
    for _ in 0..JOIN_ITERS {
        let t0 = Instant::now();
        let outcome = host
            .fire_event("player_join", &join_payload)
            .expect("bench player_join");
        let dt = t0.elapsed();
        assert!(!outcome.cancelled);
        join_samples.push(dt);
        join_elapsed_sum += dt;

        host.fire_event("player_quit", &quit_payload)
            .expect("bench player_quit");
    }
    let join_stats = summarize(&mut join_samples, join_elapsed_sum);

    // Keep BenchPlayer online for block_break events.
    host.fire_event("player_join", &join_payload)
        .expect("join for block_break");

    let block_break_payload = json!({
        "player_uuid": "00000000-0000-3000-8000-000000000001",
        "player_name": "BenchPlayer",
        "world": "minecraft:overworld",
        "x": 0,
        "y": 64,
        "z": 0,
        "material": "minecraft:stone"
    });
    for _ in 0..WARMUP_ITERS {
        host.fire_event("block_break", &block_break_payload)
            .expect("warmup block_break");
    }
    let mut block_break_samples = Vec::with_capacity(EVENT_ITERS);
    let block_break_total_start = Instant::now();
    for _ in 0..EVENT_ITERS {
        let t0 = Instant::now();
        host.fire_event("block_break", &block_break_payload)
            .expect("bench block_break");
        block_break_samples.push(t0.elapsed());
    }
    let block_break_total = block_break_total_start.elapsed();
    let block_break_stats = summarize(&mut block_break_samples, block_break_total);

    // 5. Command dispatch throughput.
    for _ in 0..WARMUP_ITERS {
        let outcome = host
            .run_command(CommandOrigin::Console, "ferrite-hello bench")
            .expect("warmup run_command");
        assert!(outcome.handled);
    }
    let mut cmd_samples = Vec::with_capacity(COMMAND_ITERS);
    let cmd_total_start = Instant::now();
    for _ in 0..COMMAND_ITERS {
        let t0 = Instant::now();
        let outcome = host
            .run_command(CommandOrigin::Console, "ferrite-hello bench")
            .expect("bench run_command");
        assert!(outcome.handled);
        cmd_samples.push(t0.elapsed());
    }
    let cmd_total = cmd_total_start.elapsed();
    let cmd_stats = summarize(&mut cmd_samples, cmd_total);

    // 6. Scheduler task execution (ticks driving typical-plugin's repeating Java task + Rust task).
    let rust_task_runs = Arc::new(AtomicU64::new(0));
    let rust_task_counter = Arc::clone(&rust_task_runs);
    let rust_task_id = host
        .schedule(0, Some(1), move || {
            rust_task_counter.fetch_add(1, Ordering::Relaxed);
        })
        .expect("scheduling repeating rust task");

    for _ in 0..WARMUP_ITERS {
        host.tick().expect("warmup tick");
    }
    let runs_before = rust_task_runs.load(Ordering::Relaxed);
    let mut tick_samples = Vec::with_capacity(TICK_ITERS);
    let tick_total_start = Instant::now();
    for _ in 0..TICK_ITERS {
        let t0 = Instant::now();
        host.tick().expect("bench tick");
        tick_samples.push(t0.elapsed());
    }
    let tick_total = tick_total_start.elapsed();
    let runs_after = rust_task_runs.load(Ordering::Relaxed);
    assert_eq!(runs_after - runs_before, TICK_ITERS as u64);
    assert!(host.cancel(rust_task_id));
    let tick_stats = summarize(&mut tick_samples, tick_total);

    let rss_after_workload = read_proc_rss_kib();

    // 7. Plugin onDisable + host shutdown latency.
    let shutdown_begin = Instant::now();
    let shutdown_report = host.shutdown();
    let shutdown_elapsed = shutdown_begin.elapsed();
    assert!(
        shutdown_report.errors.is_empty(),
        "unexpected shutdown errors: {:?}",
        shutdown_report.errors
    );
    assert_eq!(shutdown_report.disabled.len(), 2);

    println!("\n=== Ferrite Plugin Host Benchmark ===");
    println!(
        "Plugins loaded: {:?}",
        load_report
            .loaded
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );
    println!(
        "Discovery (scan_directory): {:>8.2} ms",
        discovery_elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "JVM + host start:           {:>8.2} ms",
        host_start_elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "Load + onLoad + onEnable:   {:>8.2} ms",
        load_enable_elapsed.as_secs_f64() * 1_000.0
    );
    println!(
        "onDisable + shutdown:       {:>8.2} ms",
        shutdown_elapsed.as_secs_f64() * 1_000.0
    );
    if let (Some((b_rss, _)), Some((l_rss, l_hwm)), Some((w_rss, w_hwm))) =
        (rss_before, rss_after_load, rss_after_workload)
    {
        println!(
            "RSS before start: {:.1} MiB | after load: {:.1} MiB (peak {:.1} MiB) | after workload: {:.1} MiB (peak {:.1} MiB)",
            b_rss as f64 / 1024.0,
            l_rss as f64 / 1024.0,
            l_hwm as f64 / 1024.0,
            w_rss as f64 / 1024.0,
            w_hwm as f64 / 1024.0,
        );
    } else {
        println!("RSS: unavailable on this platform (/proc/self/status not present)");
    }
    println!();
    print_stats("event:player_join", &join_stats);
    print_stats("event:block_break", &block_break_stats);
    print_stats("command:ferrite-hello", &cmd_stats);
    print_stats("scheduler:tick", &tick_stats);
    println!("=====================================\n");
}
