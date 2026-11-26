//! The process-wide plugin host: JVM ownership, plugin loading and shutdown.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ferrite_jvm::{Jvm, JvmConfig};
use ferrite_plugin_meta::{LoadPlan, LoadedPlugin};
use serde_json::{Value, json};

use crate::config::PluginHostConfig;
use crate::error::{HostError, Result};
use crate::native::{self, NativeState};
use crate::protocol::{self, CommandOrigin, CommandOutcome, EventOutcome};
use crate::scheduler::{Scheduler, TaskId};
use crate::state::HostStateProvider;

static HOST: OnceLock<PluginHost> = OnceLock::new();
static START_LOCK: Mutex<()> = Mutex::new(());

/// Which phase a plugin belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPhase {
    /// The descriptor was rejected before any JVM work.
    Discovery,
    /// `load: STARTUP`, before the world is initialized.
    Startup,
    /// `load: POSTWORLD`, after the world is ready.
    PostWorld,
}

impl PluginPhase {
    /// Contract spelling used in logs and failures.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Startup => "startup",
            Self::PostWorld => "postworld",
        }
    }
}

/// A command declared in a loaded plugin's descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCommandInfo {
    /// Command name.
    pub name: String,
    /// Aliases from `plugin.yml`.
    pub aliases: Vec<String>,
    /// Permission required to run the command.
    pub permission: Option<String>,
    /// Usage text.
    pub usage: Option<String>,
}

/// A plugin that loaded and enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Main class.
    pub main: String,
    /// Phase it was loaded in.
    pub phase: PluginPhase,
    /// Jar it came from.
    pub jar: PathBuf,
    /// Its data directory.
    pub data_folder: PathBuf,
    /// Names this plugin also answers to (`provides`).
    pub provides: Vec<String>,
    /// Commands declared in its descriptor.
    pub commands: Vec<HostCommandInfo>,
}

/// A plugin that did not load, with the phase and error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFailure {
    /// Plugin name when the descriptor could be read.
    pub plugin: Option<String>,
    /// Jar that failed.
    pub jar: PathBuf,
    /// Phase where it failed.
    pub phase: PluginPhase,
    /// Contract error code.
    pub code: String,
    /// Human-readable error, including a Java stack trace where available.
    pub error: String,
}

/// Result of loading one phase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Plugins enabled in this phase.
    pub loaded: Vec<LoadedPluginInfo>,
    /// Plugins that failed in this phase.
    pub failed: Vec<PluginFailure>,
}

/// Result of shutting the host down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Plugins disabled before the Java host closed.
    pub disabled: Vec<String>,
    /// Errors from plugins that failed to disable.
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Plan {
    pub(crate) startup: Vec<LoadedPlugin>,
    pub(crate) postworld: Vec<LoadedPlugin>,
    pub(crate) failed: Vec<PluginFailure>,
}

impl Plan {
    fn from_load_plan(plan: LoadPlan) -> Self {
        let failed = plan
            .failed
            .into_iter()
            .map(|failure| PluginFailure {
                plugin: failure.name,
                jar: failure.source,
                phase: PluginPhase::Discovery,
                code: "metadata".into(),
                error: failure.error.to_string(),
            })
            .collect();
        Self {
            startup: plan.startup,
            postworld: plan.postworld,
            failed,
        }
    }
}

#[derive(Default)]
struct LoadedState {
    loaded: Vec<LoadedPluginInfo>,
    failed: Vec<PluginFailure>,
    loaded_names: HashSet<String>,
}

pub(crate) struct HostState {
    config: PluginHostConfig,
    plan: Plan,
    jvm: Option<Jvm>,
    host_thread: Option<HostThread>,
    scheduler: Arc<Scheduler>,
    native: Option<Arc<NativeState>>,
    loaded: Mutex<LoadedState>,
    postworld_loaded: AtomicBool,
    stopped: AtomicBool,
}

/// The embedded plugin host.
///
/// Cloning is cheap: every clone refers to the same host. Starting a second host in the same
/// process fails because a JVM cannot be created twice.
#[derive(Clone)]
pub struct PluginHost {
    inner: Arc<HostState>,
}

impl PluginHost {
    /// Scans the plugins directory, starts the one embedded JVM and loads `STARTUP` plugins.
    ///
    /// A missing or empty plugins directory returns a dormant host that does not start a JVM;
    /// call [`PluginHost::is_running`] to distinguish it. `enabled = false` returns
    /// [`HostError::Disabled`] without touching the directory.
    pub fn start(config: PluginHostConfig) -> Result<Self> {
        if !config.enabled {
            return Err(HostError::Disabled);
        }
        let _start = START_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if HOST.get().is_some() {
            return Err(HostError::AlreadyStarted);
        }

        let scanned = match ferrite_plugin_meta::scan_directory(&config.plugins_dir) {
            Ok(plan) => plan,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    dir = %config.plugins_dir.display(),
                    "plugin directory is missing; no plugin host is started"
                );
                return Ok(Self::dormant(config, Plan::default()));
            }
            Err(error) => {
                return Err(HostError::Metadata(format!(
                    "reading {}: {error}",
                    config.plugins_dir.display()
                )));
            }
        };
        let plan = Plan::from_load_plan(scanned);
        for failure in &plan.failed {
            tracing::warn!(
                plugin = failure.plugin.as_deref().unwrap_or("<unknown>"),
                jar = %failure.jar.display(),
                error = %failure.error,
                "plugin metadata rejected"
            );
        }
        if plan.startup.is_empty() && plan.postworld.is_empty() {
            tracing::info!(
                failures = plan.failed.len(),
                "no plugins to load; no embedded JVM is started"
            );
            return Ok(Self::dormant(config, plan));
        }

        let classpath = host_classpath(&config)?;

        // Java 25 restricts native access unless the embedding process opts in. The host is a
        // JNI embedder by definition, so this option is always present unless the operator
        // supplied their own.
        let mut jvm_args = config.jvm_args.clone();
        if !jvm_args
            .iter()
            .any(|argument| argument.starts_with("--enable-native-access"))
        {
            jvm_args.push("--enable-native-access=ALL-UNNAMED".into());
        }

        let jvm = match Jvm::get() {
            Some(jvm) => {
                for entry in &classpath {
                    if !jvm
                        .classpath()
                        .iter()
                        .any(|existing| same_path(existing, entry))
                    {
                        return Err(HostError::JvmAlreadyRunning {
                            path: entry.clone(),
                        });
                    }
                }
                jvm
            }
            None => Jvm::start(JvmConfig {
                java_home: config.java_home.clone(),
                classpath: classpath.clone(),
                max_heap_mb: config.max_heap_mb,
                extra_args: jvm_args,
            })
            .map_err(|error| HostError::Jvm(error.to_string()))?,
        };

        let scheduler = Arc::new(Scheduler::new());
        let known = plan
            .startup
            .iter()
            .chain(plan.postworld.iter())
            .map(|plugin| plugin.name().to_owned());
        let native_state = Arc::new(NativeState::new(&config, Arc::clone(&scheduler), known));
        native::install(Arc::clone(&native_state));
        jvm.register_natives("dev/ferrite/host/HostBridge", &native::native_methods())
            .map_err(|error| HostError::Jvm(error.to_string()))?;
        let host_thread = HostThread::start("ferrite-plugin-host")?;

        let host = Self {
            inner: Arc::new(HostState {
                config: config.clone(),
                plan,
                jvm: Some(jvm),
                host_thread: Some(host_thread),
                scheduler,
                native: Some(native_state),
                loaded: Mutex::new(LoadedState::default()),
                postworld_loaded: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            }),
        };

        let config_json = protocol::config_json(&config).to_string();
        let response = host.host_call("Bootstrap.start", config.start_timeout, move || {
            protocol::call_java_string(jvm, "dev/ferrite/host/Bootstrap", "start", &config_json)
        })?;
        let value = protocol::parse_json(&response)?;
        protocol::ensure_ok(&value)?;
        let version = value.get("contract_version").and_then(Value::as_i64);
        if !matches!(
            version,
            Some(v) if (crate::MIN_CONTRACT_VERSION..=crate::CONTRACT_VERSION).contains(&v)
        ) {
            return Err(HostError::InvalidConfig(format!(
                "Java host contract version {version:?} is outside supported range {}..={}",
                crate::MIN_CONTRACT_VERSION,
                crate::CONTRACT_VERSION
            )));
        }
        if HOST.set(host.clone()).is_err() {
            return Err(HostError::AlreadyStarted);
        }

        let startup = host.load_phase(PluginPhase::Startup);
        for failure in &startup.failed {
            tracing::warn!(
                plugin = failure.plugin.as_deref().unwrap_or("<unknown>"),
                phase = failure.phase.as_str(),
                error = %failure.error,
                "plugin failed to start"
            );
        }
        Ok(host)
    }

    /// The running host, if a previous [`PluginHost::start`] succeeded.
    pub fn get() -> Option<Self> {
        HOST.get().cloned()
    }

    /// Whether a JVM-backed host is running. A host with no plugins is not running.
    pub fn is_running(&self) -> bool {
        self.inner.jvm.is_some() && !self.inner.stopped.load(Ordering::SeqCst)
    }

    /// Whether a plugins directory was found but had nothing to load.
    pub fn is_dormant(&self) -> bool {
        self.inner.jvm.is_none()
    }

    /// Loads `POSTWORLD` plugins. Repeated calls are no-ops.
    pub fn load_postworld(&self) -> LoadReport {
        if self.inner.jvm.is_none() {
            return LoadReport::default();
        }
        if self.inner.postworld_loaded.swap(true, Ordering::SeqCst) {
            return LoadReport::default();
        }
        self.load_phase(PluginPhase::PostWorld)
    }

    /// Plugins that loaded so far.
    pub fn loaded(&self) -> Vec<LoadedPluginInfo> {
        self.inner
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .loaded
            .clone()
    }

    /// Plugins that failed so far, including metadata failures from discovery.
    pub fn failures(&self) -> Vec<PluginFailure> {
        self.inner
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .failed
            .clone()
    }

    /// Commands declared by loaded plugins.
    pub fn commands(&self) -> Vec<HostCommandInfo> {
        self.loaded()
            .into_iter()
            .flat_map(|plugin| plugin.commands)
            .collect()
    }

    /// Fires one of the contract v1 events. Unknown names are an error, never a silent pass.
    pub fn fire_event(&self, name: &str, payload: &Value) -> Result<EventOutcome> {
        if !protocol::is_known_event(name) {
            return Err(HostError::UnknownEvent(name.to_owned()));
        }
        if !self.is_running() {
            return Err(HostError::NotRunning);
        }
        let jvm = self.inner.jvm.ok_or(HostError::NotRunning)?;
        let timeout = self.inner.config.call_timeout;
        let event = name.to_owned();
        let payload = payload.to_string();
        let response = self.host_call("Bootstrap.fireEvent", timeout, move || {
            protocol::call_java_strings2(
                jvm,
                "dev/ferrite/host/Bootstrap",
                "fireEvent",
                &event,
                &payload,
            )
        })?;
        protocol::parse_event_result(&response)
    }

    /// Runs due scheduled tasks and drains the state provider's accepted writes. Call once per
    /// server tick from the server tick thread.
    pub fn tick(&self) -> Result<()> {
        if !self.is_running() {
            return Ok(());
        }
        if let Some(native) = &self.inner.native {
            native.tick_provider();
        }
        if let Some(jvm) = self.inner.jvm {
            self.inner.scheduler.tick(Some(jvm));
        }
        Ok(())
    }

    /// Installs the server-owned world/region state adapter.
    ///
    /// The adapter can be installed after the host has started: STARTUP plugins load before the
    /// world exists. Until it is installed, v2 state ops fail with `state_unavailable`.
    pub fn install_state_provider(&self, provider: Arc<dyn HostStateProvider>) -> Result<()> {
        if !self.is_running() {
            return Err(HostError::NotRunning);
        }
        let native = self.inner.native.as_ref().ok_or(HostError::NotRunning)?;
        native.set_provider(provider);
        Ok(())
    }

    /// Schedules a host-internal task. Java plugin tasks use `HostContext.schedule` instead.
    pub fn schedule<F>(
        &self,
        delay_ticks: u64,
        period_ticks: Option<u64>,
        task: F,
    ) -> Result<TaskId>
    where
        F: FnMut() + Send + 'static,
    {
        if !self.is_running() {
            return Err(HostError::NotRunning);
        }
        Ok(self
            .inner
            .scheduler
            .schedule_rust(None, delay_ticks, period_ticks, Box::new(task)))
    }

    /// Cancels a task created by [`PluginHost::schedule`].
    pub fn cancel(&self, task: TaskId) -> bool {
        self.inner.scheduler.cancel(task)
    }

    /// Returns whether any enabled plugin has registered `channel` as an incoming plugin channel.
    pub fn is_incoming_channel_registered(&self, channel: &str) -> bool {
        self.inner
            .native
            .as_ref()
            .is_some_and(|native| native.is_incoming_channel_registered(channel))
    }

    /// Calls a Rust-side RPC handler directly (`host.info`, `host.ping`, ...).
    pub fn call(&self, op: &str, request: &Value) -> Result<Value> {
        if !self.is_running() {
            return Err(HostError::NotRunning);
        }
        native::call(op, request).map_err(|error| HostError::Rpc {
            code: error.code,
            message: error.message,
        })
    }

    /// Runs a command through the Java `HostApi` implementation.
    pub fn run_command(&self, origin: CommandOrigin, line: &str) -> Result<CommandOutcome> {
        if !self.is_running() {
            return Err(HostError::NotRunning);
        }
        let jvm = self.inner.jvm.ok_or(HostError::NotRunning)?;
        let timeout = self.inner.config.call_timeout;
        let origin = protocol::command_origin_json(&origin);
        let line = line.to_owned();
        let response = self.host_call("Bootstrap.runCommand", timeout, move || {
            protocol::call_java_strings2(
                jvm,
                "dev/ferrite/host/Bootstrap",
                "runCommand",
                &origin,
                &line,
            )
        })?;
        protocol::parse_command(&response)
    }

    /// Disables plugins in reverse load order, drops scheduled tasks and stops the host thread.
    ///
    /// Idempotent. A failure to disable one plugin is reported but does not stop the others.
    pub fn shutdown(&self) -> ShutdownReport {
        if self.inner.stopped.swap(true, Ordering::SeqCst) {
            return ShutdownReport::default();
        }
        if let Some(native) = &self.inner.native {
            native.mark_stopped();
            native.shutdown_provider();
        }
        let mut report = ShutdownReport::default();
        if let (Some(jvm), Some(thread)) = (self.inner.jvm, self.inner.host_thread.as_ref()) {
            let response = thread.call(
                "Bootstrap.shutdown",
                self.inner.config.call_timeout,
                move || {
                    protocol::call_java_string(
                        jvm,
                        "dev/ferrite/host/Bootstrap",
                        "shutdown",
                        "{\"reason\":\"server shutdown\"}",
                    )
                },
            );
            match response {
                Ok(text) => match protocol::parse_shutdown(&text) {
                    Ok(shutdown) => {
                        report.disabled = shutdown.disabled;
                        report.errors = shutdown.errors;
                    }
                    Err(error) => report.errors.push(error.to_string()),
                },
                Err(error) => report.errors.push(error.to_string()),
            }
        }
        self.inner.scheduler.stop();
        if let Some(thread) = &self.inner.host_thread {
            thread.shutdown();
        }
        report
    }

    /// A small JSON snapshot for diagnostics and tests.
    pub fn status(&self) -> Value {
        let loaded: Vec<Value> = self
            .loaded()
            .iter()
            .map(|plugin| {
                json!({
                    "name": &plugin.name,
                    "version": &plugin.version,
                    "phase": plugin.phase.as_str(),
                })
            })
            .collect();
        json!({
            "running": self.is_running(),
            "dormant": self.is_dormant(),
            "loaded": loaded,
            "failures": self.failures().len(),
        })
    }

    fn dormant(config: PluginHostConfig, plan: Plan) -> Self {
        let loaded = LoadedState {
            failed: plan.failed.clone(),
            ..LoadedState::default()
        };
        Self {
            inner: Arc::new(HostState {
                config,
                plan,
                jvm: None,
                host_thread: None,
                scheduler: Arc::new(Scheduler::new()),
                native: None,
                loaded: Mutex::new(loaded),
                postworld_loaded: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            }),
        }
    }

    fn load_phase(&self, phase: PluginPhase) -> LoadReport {
        let plugins = match phase {
            PluginPhase::Startup => self.inner.plan.startup.clone(),
            PluginPhase::PostWorld => self.inner.plan.postworld.clone(),
            PluginPhase::Discovery => Vec::new(),
        };
        let mut report = LoadReport::default();
        for plugin in &plugins {
            let name = plugin.name().to_owned();
            if let Some(dependency) = self.missing_dependency(plugin) {
                let failure = PluginFailure {
                    plugin: Some(name.clone()),
                    jar: plugin.source.clone(),
                    phase,
                    code: "dependency_failed".into(),
                    error: format!("required dependency '{dependency}' is not loaded"),
                };
                tracing::warn!(
                    plugin = %name,
                    phase = phase.as_str(),
                    dependency = %dependency,
                    "plugin skipped because a dependency is missing"
                );
                self.record_failure(&failure);
                report.failed.push(failure);
                continue;
            }
            if let Some(native) = &self.inner.native {
                native.register_plugin(&name);
            }
            match self.load_one(plugin, phase) {
                Ok(info) => {
                    tracing::info!(
                        plugin = %info.name,
                        version = %info.version,
                        phase = phase.as_str(),
                        "plugin enabled"
                    );
                    self.record_loaded(&info);
                    report.loaded.push(info);
                }
                Err(error) => {
                    // A plugin may have scheduled tasks before onEnable failed.
                    self.inner.scheduler.cancel_plugin(&name);
                    let failure = PluginFailure {
                        plugin: Some(name.clone()),
                        jar: plugin.source.clone(),
                        phase,
                        code: error.code().to_owned(),
                        error: error.to_string(),
                    };
                    tracing::warn!(
                        plugin = %name,
                        phase = phase.as_str(),
                        error = %failure.error,
                        "plugin failed to load"
                    );
                    self.record_failure(&failure);
                    report.failed.push(failure);
                }
            }
        }
        report
    }

    fn missing_dependency(&self, plugin: &LoadedPlugin) -> Option<String> {
        let loaded = self
            .inner
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut required = plugin.meta.depend.clone();
        if let Some(paper) = &plugin.meta.paper {
            for (name, dependency) in &paper.dependencies.server {
                if dependency.required && dependency.join_classpath {
                    required.push(name.clone());
                }
            }
        }
        let own_name = plugin.meta.name.to_ascii_lowercase();
        required.into_iter().find(|dependency| {
            let key = dependency.to_ascii_lowercase();
            key != own_name && !loaded.loaded_names.contains(&key)
        })
    }

    fn load_one(&self, plugin: &LoadedPlugin, phase: PluginPhase) -> Result<LoadedPluginInfo> {
        let data_folder = self.data_folder(plugin.name())?;
        let payload = protocol::plugin_json(plugin, &data_folder)?;
        let jvm = self.inner.jvm.ok_or(HostError::NotRunning)?;
        let timeout = self.inner.config.call_timeout;
        let name = plugin.name().to_owned();
        let response = self.host_call("Bootstrap.loadPlugin", timeout, move || {
            protocol::call_java_string(jvm, "dev/ferrite/host/Bootstrap", "loadPlugin", &payload)
        })?;
        let parsed = protocol::parse_plugin_load(&response)?;
        if parsed.ok {
            let commands = plugin
                .meta
                .commands
                .iter()
                .map(|(name, command)| HostCommandInfo {
                    name: name.clone(),
                    aliases: command.aliases.clone(),
                    permission: command.permission.clone(),
                    usage: command.usage.clone(),
                })
                .collect();
            Ok(LoadedPluginInfo {
                name,
                version: plugin.meta.version.clone(),
                main: plugin.meta.main.clone(),
                phase,
                jar: plugin.source.clone(),
                data_folder,
                provides: plugin.meta.provides.clone(),
                commands,
            })
        } else {
            Err(HostError::PluginFailed {
                plugin: name,
                phase: parsed.phase,
                message: parsed.message,
            })
        }
    }

    fn data_folder(&self, name: &str) -> Result<PathBuf> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\\')
        {
            return Err(HostError::InvalidConfig(format!(
                "plugin name '{name}' cannot be used as a data directory"
            )));
        }
        let folder = absolute(&self.inner.config.data_dir.join(name))?;
        std::fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    fn record_loaded(&self, info: &LoadedPluginInfo) {
        let mut loaded = self
            .inner
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for name in std::iter::once(&info.name).chain(info.provides.iter()) {
            loaded.loaded_names.insert(name.to_ascii_lowercase());
        }
        loaded.loaded.push(info.clone());
    }

    fn record_failure(&self, failure: &PluginFailure) {
        self.inner
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .failed
            .push(failure.clone());
    }

    fn host_call<T: Send + 'static>(
        &self,
        label: &'static str,
        timeout: Duration,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.inner
            .host_thread
            .as_ref()
            .ok_or(HostError::NotRunning)?
            .call(label, timeout, work)
    }
}

/// Validates and orders the host classpath: the host jar first, then the API runtime jars in
/// configured order. Plugin jars are never on this classpath.
fn host_classpath(config: &PluginHostConfig) -> Result<Vec<PathBuf>> {
    let host_jar = absolute(&config.host_jar)?;
    if !host_jar.exists() {
        return Err(HostError::HostJarMissing { path: host_jar });
    }
    let mut classpath = vec![host_jar];
    for entry in &config.runtime_classpath {
        let entry = absolute(entry)?;
        if !entry.exists() {
            return Err(HostError::HostJarMissing { path: entry });
        }
        classpath.push(entry);
    }
    Ok(classpath)
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).map_err(HostError::from)
}

fn same_path(first: &Path, second: &Path) -> bool {
    if first == second {
        return true;
    }
    match (std::fs::canonicalize(first), std::fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;

struct HostThread {
    jobs: Mutex<Option<Sender<Job>>>,
}

impl HostThread {
    fn start(name: &str) -> Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || run_jobs(receiver))?;
        Ok(Self {
            jobs: Mutex::new(Some(sender)),
        })
    }

    fn call<T: Send + 'static>(
        &self,
        label: &'static str,
        timeout: Duration,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let sender = self
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .cloned()
            .ok_or(HostError::HostThreadStopped)?;
        sender
            .send(Box::new(move || {
                let _ = result_tx.send(work());
            }))
            .map_err(|_| HostError::HostThreadStopped)?;
        match result_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(HostError::Timeout {
                operation: label,
                millis: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::HostThreadStopped),
        }
    }

    fn shutdown(&self) {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}

impl Drop for HostThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_jobs(receiver: Receiver<Job>) {
    while let Ok(job) = receiver.recv() {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
            tracing::error!("a plugin host job panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_error_and_does_not_touch_the_directory() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("does-not-exist");
        let config = PluginHostConfig {
            enabled: false,
            plugins_dir: missing.clone(),
            data_dir: missing.clone(),
            ..PluginHostConfig::default()
        };
        let error = PluginHost::start(config)
            .err()
            .expect("disabled host fails");
        assert!(matches!(error, HostError::Disabled));
        assert!(!missing.exists());
    }

    #[test]
    fn missing_directory_returns_a_dormant_host() {
        let directory = tempfile::tempdir().unwrap();
        let config = PluginHostConfig::new(directory.path().join("plugins"));
        let host = PluginHost::start(config).expect("missing directory is not fatal");
        assert!(host.is_dormant());
        assert!(!host.is_running());
        assert!(host.loaded().is_empty());
        assert!(host.load_postworld().loaded.is_empty());
        host.shutdown();
    }

    #[test]
    fn host_thread_timeout_is_a_rust_error() {
        let thread = HostThread::start("ferrite-plugin-test").unwrap();
        let error = thread
            .call("test", Duration::from_millis(20), || {
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(error, HostError::Timeout { .. }));
        thread.shutdown();
    }

    #[test]
    fn classpath_puts_the_host_jar_first_and_keeps_runtime_order() {
        let directory = tempfile::tempdir().unwrap();
        let host_jar = directory.path().join("ferrite-host.jar");
        let api_impl = directory.path().join("ferrite-api-impl.jar");
        let nms = directory.path().join("ferrite-nms.jar");
        for path in [&host_jar, &api_impl, &nms] {
            std::fs::write(path, b"jar").unwrap();
        }
        let config = PluginHostConfig {
            host_jar: host_jar.clone(),
            runtime_classpath: vec![api_impl.clone(), nms.clone()],
            ..PluginHostConfig::default()
        };
        assert_eq!(
            host_classpath(&config).unwrap(),
            vec![host_jar, api_impl, nms]
        );
    }

    #[test]
    fn event_names_are_closed() {
        assert!(protocol::is_known_event("player_join"));
        assert!(!protocol::is_known_event("server_start"));
    }

    #[test]
    fn data_folder_rejects_path_traversal() {
        let host = PluginHost::start(PluginHostConfig {
            enabled: true,
            plugins_dir: tempfile::tempdir().unwrap().path().join("plugins"),
            ..PluginHostConfig::default()
        })
        .unwrap();
        let error = host.data_folder("../escape").unwrap_err();
        assert!(matches!(error, HostError::InvalidConfig(_)));
    }
}
