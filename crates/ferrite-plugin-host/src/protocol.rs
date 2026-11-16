//! JSON shapes and JNI calls for contract v1.
//!
//! Everything here is synchronous: the caller decides which thread runs it and how long to
//! wait (see [`crate::host`]). Java exceptions become [`HostError::JavaException`] with the
//! stack trace captured by [`ferrite_jvm::Jvm::with_env`].

use std::path::Path;

use ferrite_jvm::Jvm;
use ferrite_jvm::jni::JNIEnv;
use ferrite_jvm::jni::objects::{JString, JValue, JValueOwned};
use ferrite_plugin_meta::LoadedPlugin;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::PluginHostConfig;
use crate::error::{HostError, Result};

/// Event names supported by contract v1.
pub const EVENTS: [&str; 4] = [
    "player_join",
    "player_quit",
    "block_break",
    "player_interact",
];

/// Whether `name` is one of the v1 events.
pub fn is_known_event(name: &str) -> bool {
    EVENTS.contains(&name)
}

/// A failed Java→Rust RPC, ready to be serialized as the contract error envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    /// Contract error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl RpcError {
    /// Builds an error with a contract code.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    /// `invalid_request`: the request JSON does not match the schema.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    /// `not_running`: the host has not started or is shutting down.
    pub fn not_running() -> Self {
        Self::new("not_running", "the plugin host is not running")
    }
}

/// Who ran a plugin command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOrigin {
    /// The server console.
    Console,
    /// A player.
    Player {
        /// Player UUID as text.
        uuid: String,
        /// Player name.
        name: String,
    },
}

/// One message a plugin command sent to a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostMessage {
    /// Recipient UUID, or `None` for a console message.
    pub player: Option<String>,
    /// Message text (plain text or component JSON, see `adventure`).
    pub content: String,
    /// Whether `content` is Adventure component JSON.
    pub adventure: bool,
}

/// Result of running a plugin command through the Java SPI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutcome {
    /// Whether a plugin handled the command.
    pub handled: bool,
    /// Messages captured from `CommandSender.sendMessage`.
    pub messages: Vec<HostMessage>,
}

/// Result of firing one event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventOutcome {
    /// Whether a listener cancelled the event. Only `block_break` and `player_interact` can.
    pub cancelled: bool,
}

/// Request of `scheduler.schedule`.
#[derive(Debug, Deserialize)]
pub struct ScheduleRequest {
    /// Plugin that owns the task.
    pub plugin: String,
    /// Opaque Java `TaskRegistry` handle.
    pub handle: i64,
    /// Delay in server ticks. `0` runs on the next tick.
    #[serde(default)]
    pub delay_ticks: u64,
    /// Repetition period in ticks. `0` is a one-shot task.
    #[serde(default)]
    pub period_ticks: u64,
    /// Run on the host's async executor instead of the tick thread.
    #[serde(default, rename = "async")]
    pub is_async: bool,
}

/// Request of `scheduler.cancel`.
#[derive(Debug, Deserialize)]
pub struct CancelRequest {
    /// Plugin that owns the task.
    pub plugin: String,
    /// Task id returned by `scheduler.schedule`.
    pub task_id: u64,
}

/// Parsed `Bootstrap.loadPlugin` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLoadResponse {
    /// Whether the plugin loaded and enabled.
    pub ok: bool,
    /// Java phase reported on failure (`load`, `enable`, `api`).
    pub phase: String,
    /// Failure text including the stack trace.
    pub message: String,
}

/// Parsed `Bootstrap.runTask`/`runTaskAsync` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTaskResponse {
    /// The task ran (or was submitted) and should run again.
    Keep,
    /// The task ran and is gone.
    Done,
    /// Java no longer knows the handle.
    Missing,
    /// The task failed and was removed.
    Failed(String),
}

/// Parsed `Bootstrap.shutdown` response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShutdownResponse {
    /// Plugins disabled, in the order Java disabled them.
    pub disabled: Vec<String>,
    /// `name: error` entries for plugins that failed to disable.
    pub errors: Vec<String>,
}

/// Converts a path to the absolute string form sent over the bridge.
pub(crate) fn path_string(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The `Bootstrap.start` configuration JSON.
pub(crate) fn config_json(config: &PluginHostConfig) -> Value {
    json!({
        "contract_version": crate::CONTRACT_VERSION,
        "api_version": config.api_version.as_str(),
        "server": {
            "name": config.server.name.as_str(),
            "version": config.server.version.as_str(),
            "motd": config.server.motd.as_str(),
            "max_players": config.server.max_players,
            "online_mode": config.server.online_mode,
            "view_distance": config.server.view_distance,
        },
        "plugins_dir": path_string(&config.plugins_dir),
        "data_dir": path_string(&config.data_dir),
        "api_impl_class": config.api_impl_class.as_deref(),
    })
}

/// The `Bootstrap.loadPlugin` metadata JSON for one plugin.
pub(crate) fn plugin_json(plugin: &LoadedPlugin, data_folder: &Path) -> Result<String> {
    let meta = &plugin.meta;
    let mut commands = Map::new();
    for (name, command) in &meta.commands {
        commands.insert(
            name.clone(),
            json!({
                "description": command.description.as_deref(),
                "usage": command.usage.as_deref(),
                "aliases": &command.aliases,
                "permission": command.permission.as_deref(),
            }),
        );
    }
    let mut permissions = Map::new();
    for (node, permission) in &meta.permissions {
        permissions.insert(
            node.clone(),
            json!({
                "description": permission.description.as_deref(),
                "default": permission.default.as_str(),
                "children": &permission.children,
            }),
        );
    }
    let value = json!({
        "name": meta.name.as_str(),
        "version": meta.version.as_str(),
        "main": meta.main.as_str(),
        "api_version": meta.api_version.as_deref(),
        "load": meta.load.as_str(),
        "dependencies": &meta.depend,
        "soft_dependencies": &meta.softdepend,
        "load_before": &meta.loadbefore,
        "provides": &meta.provides,
        "folia_supported": meta.folia_supported,
        "format": meta.format.file_name(),
        "jar": path_string(&plugin.source),
        "data_folder": path_string(data_folder),
        "commands": commands,
        "permissions": permissions,
    });
    Ok(serde_json::to_string(&value)?)
}

/// The `Bootstrap.runCommand` origin JSON.
pub(crate) fn command_origin_json(origin: &CommandOrigin) -> String {
    let value = match origin {
        CommandOrigin::Console => json!({"type": "console"}),
        CommandOrigin::Player { uuid, name } => {
            json!({"type": "player", "uuid": uuid, "name": name})
        }
    };
    value.to_string()
}

pub(crate) fn parse_json(response: &str) -> Result<Value> {
    serde_json::from_str(response).map_err(HostError::from)
}

/// Renders a Java failure (`error` + optional `stack`) for Rust logs.
pub(crate) fn error_message(value: &Value) -> String {
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("the Java plugin host reported a failure");
    match value.get("stack").and_then(Value::as_str) {
        Some(stack) if !stack.is_empty() => format!("{error}\n{stack}"),
        _ => error.to_owned(),
    }
}

/// Fails when the Java host returned `{ok:false,...}`.
pub(crate) fn ensure_ok(value: &Value) -> Result<()> {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(HostError::JavaException(error_message(value)))
    }
}

/// Parses the response of `Bootstrap.loadPlugin`.
pub(crate) fn parse_plugin_load(response: &str) -> Result<PluginLoadResponse> {
    let value = parse_json(response)?;
    let ok = value.get("ok").and_then(Value::as_bool) == Some(true);
    Ok(PluginLoadResponse {
        ok,
        phase: value
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("load")
            .to_owned(),
        message: if ok {
            String::new()
        } else {
            error_message(&value)
        },
    })
}

/// Parses the response of `Bootstrap.fireEvent`.
pub(crate) fn parse_event_result(response: &str) -> Result<EventOutcome> {
    let value = parse_json(response)?;
    ensure_ok(&value)?;
    Ok(EventOutcome {
        cancelled: value
            .get("cancelled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Parses the response of `Bootstrap.runTask` and `Bootstrap.runTaskAsync`.
pub(crate) fn parse_run_task(response: &str) -> Result<RunTaskResponse> {
    let value = parse_json(response)?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        if value.get("missing").and_then(Value::as_bool) == Some(true) {
            return Ok(RunTaskResponse::Missing);
        }
        return Ok(RunTaskResponse::Failed(error_message(&value)));
    }
    if value.get("repeat").and_then(Value::as_bool) == Some(true) {
        Ok(RunTaskResponse::Keep)
    } else {
        Ok(RunTaskResponse::Done)
    }
}

/// Parses the response of `Bootstrap.shutdown`.
pub(crate) fn parse_shutdown(response: &str) -> Result<ShutdownResponse> {
    let value = parse_json(response)?;
    ensure_ok(&value)?;
    let disabled = value
        .get("disabled")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let errors = value
        .get("failed")
        .and_then(Value::as_array)
        .map(|failures| {
            failures
                .iter()
                .map(|failure| {
                    let name = failure
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let error = failure
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error");
                    format!("{name}: {error}")
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ShutdownResponse { disabled, errors })
}

/// Parses the response of `Bootstrap.runCommand`.
pub(crate) fn parse_command(response: &str) -> Result<CommandOutcome> {
    let value = parse_json(response)?;
    ensure_ok(&value)?;
    let handled = value
        .get("handled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| {
                    Some(HostMessage {
                        player: message
                            .get("player")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        content: message.get("content").and_then(Value::as_str)?.to_owned(),
                        adventure: message
                            .get("adventure")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(CommandOutcome { handled, messages })
}

fn jvm_error(error: ferrite_jvm::JvmError) -> HostError {
    match error {
        ferrite_jvm::JvmError::JavaException(text) => HostError::JavaException(text),
        other => HostError::Jvm(other.to_string()),
    }
}

fn read_string(
    env: &mut JNIEnv<'_>,
    value: JValueOwned<'_>,
) -> ferrite_jvm::jni::errors::Result<String> {
    let object = value.l()?;
    let string = JString::from(object);
    Ok(env.get_string(&string)?.into())
}

/// Calls `class.method(String) -> String`.
pub(crate) fn call_java_string(
    jvm: Jvm,
    class: &str,
    method: &str,
    argument: &str,
) -> Result<String> {
    jvm.with_env(|env| {
        let argument = env.new_string(argument)?;
        let value = env.call_static_method(
            class,
            method,
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&argument)],
        )?;
        read_string(env, value)
    })
    .map_err(jvm_error)
}

/// Calls `class.method(String, String) -> String`.
pub(crate) fn call_java_strings2(
    jvm: Jvm,
    class: &str,
    method: &str,
    first: &str,
    second: &str,
) -> Result<String> {
    jvm.with_env(|env| {
        let first = env.new_string(first)?;
        let second = env.new_string(second)?;
        let value = env.call_static_method(
            class,
            method,
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&first), JValue::Object(&second)],
        )?;
        read_string(env, value)
    })
    .map_err(jvm_error)
}

/// Calls `class.method(long) -> String`.
pub(crate) fn call_java_long(jvm: Jvm, class: &str, method: &str, value: i64) -> Result<String> {
    jvm.with_env(|env| {
        let value = env.call_static_method(
            class,
            method,
            "(J)Ljava/lang/String;",
            &[JValue::Long(value)],
        )?;
        read_string(env, value)
    })
    .map_err(jvm_error)
}
