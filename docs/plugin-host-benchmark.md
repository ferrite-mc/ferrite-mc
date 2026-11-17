# Ferrite Plugin Host Benchmark Baseline

This document records the repeatable baseline benchmark for Ferrite's embedded JVM plugin host (`ferrite-plugin-host` + `java/ferrite-host` + `java/ferrite-api-impl` + `java/ferrite-nms`) running the real `typical-plugin` and `nms-plugin` fixtures.

> [!NOTE]
> This is an initial baseline measurement, not a before/after comparison. No speedup is claimed. No real Minecraft client is used.

## Environment

- **Date**: 2026-11-17
- **OS**: Linux (x86_64, KVM)
- **CPU**: Intel(R) Xeon(R) Platinum 8358 CPU @ 2.60GHz (12 vCPUs, 6 cores / 12 threads)
- **RAM**: 62 GiB (no swap)
- **JVM**: OpenJDK 25.0.5.0.3 (64-Bit Server VM, mixed mode, sharing)
- **Rust**: `cargo 1.100.0 (3acc79c0c 2026-11-05)` (`--release` profile: `lto = "fat"`, `codegen-units = 1`)
- **Heap limit**: `-Xmx512m` (`max_heap_mb: Some(512)`)

## Prerequisites

The benchmark starts the real embedded JVM and loads the real fixture jars; nothing is mocked or faked. Before running:

1. **Java 25+ JDK** must be discoverable (`JAVA_HOME` or `PATH`).
2. **Vanilla 26.3 generated data** must exist at `vanilla/26.3/generated/` (or `FERRITE_VANILLA_GENERATED` must point to it) so `FerriteRegistry` can populate block/item/jukebox registries. Generate it with:
   ```sh
   python3 tools/fetch_vanilla.py
   ```
3. **Java artifacts** (`ferrite-host`, `ferrite-api-impl`, `ferrite-nms`, `typical-plugin`, `nms-plugin`, and `paper-api-classpath.txt`) are built automatically by the harness via `java/gradlew` if missing, or can be built explicitly:
   ```sh
   (cd java && ./gradlew :ferrite-host:jar :ferrite-api-impl:jar :ferrite-api-impl:printApiClasspath :ferrite-nms:jar :fixtures:typical-plugin:jar :fixtures:nms-plugin:jar --no-daemon)
   ```

## Repeatable Command

```sh
cargo test -p ferrite-plugin-host --release --test plugin_host_benchmark -- --ignored --nocapture
```

Harness source: [`crates/ferrite-plugin-host/tests/plugin_host_benchmark.rs`](../crates/ferrite-plugin-host/tests/plugin_host_benchmark.rs).

## Baseline Measurements

### Lifecycle & Process Memory (RSS)

Measured once per process (JNI allows one embedded JVM per process):

| Metric | Measured |
|---|---|
| Plugin discovery (`ferrite_plugin_meta::scan_directory`, 2 jars) | `0.18 ms` |
| JVM + host start (`PluginHost::start`, `Bootstrap.start`, `RegisterNatives`) | `234.28 ms` |
| Plugin load + `onLoad` + `onEnable` (`load_postworld`, `FerriteNmsFixture` + `TypicalFixture`) | `770.71 ms` |
| Plugin `onDisable` + shutdown (`PluginHost::shutdown`) | `3.82 ms` |
| Process RSS before JVM start (`/proc/self/status` `VmRSS`) | `3.0 MiB` |
| Process RSS after plugin load (`VmRSS` / `VmHWM` peak) | `114.2 MiB` (`147.5 MiB` peak) |
| Process RSS after workload (`VmRSS` / `VmHWM` peak) | `185.3 MiB` (`185.3 MiB` peak) |

### Throughput & Latency

All throughput benchmarks run 50 warmup iterations before the measured window.

| Operation | Iterations | Total (ms) | Median (us) | p95 (us) | Min (us) | Max (us) | Throughput (ops/s) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `event:player_join` (paired with `player_quit`) | 200 | 294.02 | 1407.66 | 2109.88 | 904.14 | 5949.66 | 680.2 |
| `event:block_break` | 500 | 73.17 | 128.41 | 178.91 | 92.09 | 5444.95 | 6833.0 |
| `command:ferrite-hello` | 500 | 110.32 | 161.53 | 335.50 | 105.70 | 3333.63 | 4532.1 |
| `scheduler:tick` (Rust repeating + `TypicalFixture` Java repeating task every 2 ticks) | 500 | 24.23 | 48.13 | 87.35 | 4.96 | 5069.50 | 20634.2 |

## Observed Bottlenecks & Proposed Optimizations (Not Implemented)

1. **Per-call `FindClass` / `GetStaticMethodID` and per-call `mpsc::channel` in `host_call`**:
   - *Observation*: `protocol::call_java_string`, `call_java_strings2`, and `call_java_long` in `crates/ferrite-plugin-host/src/protocol.rs` pass `"dev/ferrite/host/Bootstrap"` by string to `env.call_static_method(...)` on every event, command, and task tick, triggering JNI class/method lookup each time. Additionally, `PluginHost::host_call` in `crates/ferrite-plugin-host/src/host.rs` allocates a new `mpsc::channel()` per call to hop onto the `ferrite-plugin-host` thread.
   - *Proposal*: Cache a `GlobalRef` to `dev/ferrite/host/Bootstrap` and its `JStaticMethodID`s once at `PluginHost::start`, and reuse a pre-allocated response slot/oneshot or ring buffer on the `HostThread` instead of allocating a fresh `mpsc::channel` per call.

2. **JSON string marshaling across JNI on every hot-path call**:
   - *Observation*: Every `fire_event`, `run_command`, and `runTask` serializes a `serde_json::Value` to `String` in Rust, converts UTF-8 to UTF-16 `jstring`, parses JSON in Java (`MiniJson`), serializes a JSON response `jstring`, copies it back to Rust, and parses it with `serde_json`. Even `runTask(long)` pays JSON serialization/parsing overhead for `{"ok":true,"repeat":false}`.
   - *Proposal*: Add typed/binary fast-path JNI entrypoints (e.g., returning packed `jint`/`jboolean` status codes for `runTask` and `fireEvent` when no error/messages occur, or passing a reusable direct `ByteBuffer` for event payloads) while keeping the JSON contract for lifecycle/cold ops.

3. **Unbatched per-task JNI calls and O(N) task scan in `Scheduler::tick`**:
   - *Observation*: `Scheduler::tick` in `crates/ferrite-plugin-host/src/scheduler.rs` scans all entries in `BTreeMap<u64, Task>` every tick and makes a separate `call_java_long(jvm, ..., "runTask", handle)` JNI call for each due Java task.
   - *Proposal*: Index pending tasks by `due` tick (e.g. `BinaryHeap` or `BTreeMap<u64, Vec<TaskId>>`) and add a `Bootstrap.runTasks(long[] handles)` batch entrypoint so multiple due Java tasks in one tick cross JNI once.

4. **Fixture `player_join` cost (`1407.66 us` median vs `128.41 us` for `block_break`)**:
   - *Observation*: `player_join` is ~11x slower than `block_break` because `TypicalListener` and `NmsFixturePlugin` perform synchronous `Files.writeString` log appends, inventory/GUI probes, and `CompletableFuture.get(...)` round-trips back through `HostBridge.call` on every join. This reflects fixture listener work rather than pure host dispatch overhead.

## Limitations

- `PluginHost::tick()` is driven in a tight loop with a lightweight in-memory `BenchStateProvider` (`crates/ferrite-plugin-host/tests/plugin_host_benchmark.rs`), not a full `ferrite-server` world/region tick loop.
- No real Minecraft client or network connection is exercised.
- `typical-plugin` and `nms-plugin` are clean-room test fixtures; third-party production plugins will have different classloading, memory, and listener profiles.
