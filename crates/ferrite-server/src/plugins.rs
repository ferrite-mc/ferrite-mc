//! Loading plugins into the embedded JVM at server startup.
mod appearance;
mod blocks;
mod cooldown;
mod extra;
pub mod facts;
mod inventory;
mod items;
mod messaging;
mod motion;
mod network;
mod survival;
mod visual;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use ferrite_plugin::{PluginConfig, PluginHost, ServerHooks, ServerSnapshot, WorldSnapshot};

/// Everything the plugin bridge needs from the server's startup configuration.
pub struct Settings {
    pub directory: PathBuf,
    pub bridge_jar: PathBuf,
    /// Extracted vanilla data pack the bridge reads recipe and tag tables from.
    pub data_dir: PathBuf,
    /// Host part of the listener address, for `Server#getIp`.
    pub bind_address: String,
    pub motd: String,
    pub max_players: u16,
    pub port: u16,
    /// Effective view distance, already clamped to the client's 2..=32 range.
    pub view_distance: u8,
    pub online_mode: bool,
    pub whitelist: bool,
    /// The main world, published before plugins can query the server.
    pub world: WorldSnapshot,
    /// Receives player mutations a plugin made through the Bukkit API. The game loop routes
    /// them to the region that owns the player.
    pub player_handler: Option<ferrite_plugin::PlayerHandler>,
    /// Read-only world access for the Bukkit block API.
    pub world_view: Option<std::sync::Arc<dyn ferrite_plugin::WorldView>>,
    /// Receives the console commands that carry out plugin world and server mutations
    /// (`World#setBlock`, IP bans, the whitelist toggle); see [`forward_console`].
    pub console: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

/// A running embedded JVM with the server's plugins enabled.
pub struct PluginBridge {
    host: Arc<PluginHost>,
}

impl PluginBridge {
    /// Start the JVM and enable every plugin jar in `settings.directory`.
    ///
    /// Returns `None` when the directory is absent or holds no jars, so a server
    /// without plugins never pays for JVM startup.
    pub fn start(settings: Settings) -> anyhow::Result<Option<Self>> {
        if !has_plugin_jars(&settings.directory) {
            return Ok(None);
        }
        let player_commands = settings.player_handler.clone();
        let host = Arc::new(
            PluginHost::start(
                &settings.directory,
                PluginConfig {
                    bridge_jar: settings.bridge_jar.clone(),
                    data_dir: settings.data_dir,
                    server_snapshot: ServerSnapshot {
                        motd: settings.motd,
                        max_players: settings.max_players,
                        port: settings.port,
                        view_distance: settings.view_distance,
                        whitelist: settings.whitelist,
                        online_mode: settings.online_mode,
                        banned_ips: Vec::new(),
                        bind_address: settings.bind_address,
                    },
                    world: Some(settings.world.clone()),
                    player_handler: settings.player_handler,
                    world_view: settings.world_view,
                    world_handler: settings.console.clone().map(|console| {
                        let handler: ferrite_plugin::WorldHandler =
                            Arc::new(move |command| match world_console_line(&command) {
                                Some(line) => {
                                    let _ = console.send(line);
                                }
                                None => tracing::debug!(
                                    ?command,
                                    "plugin world command for a world the server does not own"
                                ),
                            });
                        handler
                    }),
                    server_handler: settings.console.clone().map(|console| {
                        let handler: ferrite_plugin::ServerHandler = Arc::new(move |command| {
                            let _ = console.send(server_console_line(&command));
                        });
                        handler
                    }),
                    host_call_handler: settings.console.map(|console| {
                        let players = player_commands;
                        let abilities = std::sync::Mutex::new(std::collections::HashMap::<
                            uuid::Uuid,
                            AbilityState,
                        >::new());
                        let appearances = std::sync::Mutex::new(std::collections::HashMap::<
                            uuid::Uuid,
                            appearance::AppearanceState,
                        >::new());
                        let cooldowns = std::sync::Mutex::new(cooldown::Cooldowns::default());
                        let handler: ferrite_plugin::HostCallHandler = Arc::new(move |call| {
                            if let Some(reply) = items::item_type_answer(call) {
                                return Some(reply);
                            }
                            if let Some(reply) = facts::answer(call) {
                                return Some(reply);
                            }
                            if call.owner == "Player"
                                && let Some((command, reply)) = cooldowns
                                    .lock()
                                    .unwrap()
                                    .handle(call, std::time::Instant::now())
                            {
                                if let (Some(players), Some(command)) = (&players, command) {
                                    players(command);
                                }
                                return Some(reply);
                            }
                            if let (Some(players), Ok(player)) =
                                (&players, uuid::Uuid::parse_str(&call.subject))
                            {
                                let mut all = appearances.lock().unwrap();
                                let state = all.entry(player).or_default();
                                let mut next = *state;
                                if let Some(command) =
                                    appearance::appearance_command(&mut next, call)
                                {
                                    *state = next;
                                    players(command);
                                    return Some(String::new());
                                }
                            }
                            if let (Some(players), Ok(player)) =
                                (&players, uuid::Uuid::parse_str(&call.subject))
                            {
                                let mut all = abilities.lock().unwrap();
                                let state = all.entry(player).or_default();
                                let mut next = *state;
                                if apply_ability(&mut next, call) {
                                    *state = next;
                                    players(ferrite_plugin::PlayerCommand::Abilities {
                                        player,
                                        allow_flight: next.allow_flight,
                                        flying: next.flying,
                                        invulnerable: next.invulnerable,
                                        fly_speed: next.fly_speed,
                                        walk_speed: next.walk_speed,
                                    });
                                    return Some("true".to_owned());
                                }
                            }
                            if let (Some(players), Ok(player)) =
                                (&players, uuid::Uuid::parse_str(&call.subject))
                            {
                                let events = game_events(call);
                                if !events.is_empty() {
                                    for (event, value) in events {
                                        players(ferrite_plugin::PlayerCommand::GameEvent {
                                            player,
                                            event,
                                            value,
                                        });
                                    }
                                    return Some(String::new());
                                }
                            }
                            if let (Some(players), Ok(player), Some(amount)) = (
                                &players,
                                uuid::Uuid::parse_str(&call.subject),
                                damage_amount(call),
                            ) {
                                players(ferrite_plugin::PlayerCommand::Damage { player, amount });
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, network::network_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, survival::survival_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, visual::visual_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, motion::motion_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, blocks::block_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, inventory::inventory_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            if let (Some(players), Some(command)) =
                                (&players, messaging::message_command(call))
                            {
                                players(command);
                                return Some(String::new());
                            }
                            match host_action(call)? {
                                HostAction::Console { line, reply } => {
                                    let _ = console.send(line);
                                    Some(reply)
                                }
                                HostAction::Reply(reply) => Some(reply),
                            }
                        });
                        handler
                    }),
                    ..Default::default()
                },
            )
            .with_context(|| format!("loading plugins from {}", settings.directory.display()))?,
        );
        let bridge = Self { host };
        bridge.flush_logs();
        Ok(Some(bridge))
    }

    /// Server-side hooks that deliver lifecycle events to plugin listeners.
    pub fn hooks(&self) -> Arc<dyn ServerHooks> {
        let hooks: Arc<dyn ServerHooks> = self.host.clone();
        hooks
    }

    /// Disable every plugin, which runs their `onDisable` handlers.
    pub fn shutdown(self) -> anyhow::Result<()> {
        let result = self.host.shutdown();
        self.flush_logs();
        result.context("disabling plugins")
    }

    /// Forward plugin log messages to the server's tracing output.
    pub fn flush_logs(&self) {
        for line in self.host.drain_logs() {
            tracing::info!(target: "ferrite::plugin", "{line}");
        }
    }
}

/// Live world reads for the Bukkit block API, backed by the server's chunk source.
struct SourceWorldView {
    source: Arc<crate::game::AnvilChunkSource>,
}

impl SourceWorldView {
    fn known(&self, world: uuid::Uuid) -> bool {
        world == ferrite_plugin::default_world_id()
    }
}

impl ferrite_plugin::WorldView for SourceWorldView {
    fn block(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String> {
        if !self.known(world) {
            return None;
        }
        let state = u16::try_from(self.source.block(x, y, z)?).ok()?;
        ferrite_data::blocks::state(state).map(|(name, _)| name.to_owned())
    }
    fn sky_light(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<u8> {
        self.known(world)
            .then(|| self.source.sky_light(x, y, z))
            .flatten()
    }
    fn highest_block(&self, world: uuid::Uuid, x: i32, z: i32) -> Option<i32> {
        self.known(world)
            .then(|| self.source.highest_block_y(x, z))
            .flatten()
    }
    fn biome(&self, world: uuid::Uuid, x: i32, y: i32, z: i32) -> Option<String> {
        self.known(world)
            .then(|| self.source.biome_name(x, y, z))
            .flatten()
    }
}

/// Build the live world view handed to the plugin bridge.
pub fn world_view(
    source: Arc<crate::game::AnvilChunkSource>,
) -> Arc<dyn ferrite_plugin::WorldView> {
    Arc::new(SourceWorldView { source })
}

/// The vanilla command that performs a plugin's world mutation, or `None` for another world.
///
/// Bukkit `Block#setType` places the material's default state, exactly like `/setblock`.
pub fn world_console_line(command: &ferrite_plugin::WorldCommand) -> Option<String> {
    match command {
        ferrite_plugin::WorldCommand::SetBlock {
            world,
            position,
            material,
        } => (*world == ferrite_plugin::default_world_id()).then(|| {
            format!(
                "setblock {} {} {} {material}",
                position.x, position.y, position.z
            )
        }),
    }
}

/// A namespaced resource key such as `minecraft:speed`; refuses anything a command could misread.
fn is_resource_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '_' | '-' | '.' | '/')
        })
}

/// The world a `World#...` call targets, only when it is the world the server's commands act on.
fn default_world(subject: &str) -> Option<()> {
    (uuid::Uuid::parse_str(subject).ok()? == ferrite_plugin::default_world_id()).then_some(())
}

/// Parses an encoded `Location` (`world, x, y, z, yaw, pitch`) into its world id and block position.
fn location_block(encoded: &str) -> Option<(uuid::Uuid, i64, i64, i64)> {
    let mut parts = encoded.split(ferrite_plugin::HOST_ITEM_SEPARATOR);
    let world = parts.next()?;
    let mut coordinate = || -> Option<i64> {
        let value: f64 = parts.next()?.parse().ok()?;
        value.is_finite().then(|| value.floor() as i64)
    };
    let (x, y, z) = (coordinate()?, coordinate()?, coordinate()?);
    // Bukkit encodes a location's world by name; only the default world has a console target.
    let world = if world == "world" {
        ferrite_plugin::default_world_id()
    } else {
        uuid::Uuid::parse_str(world).ok()?
    };
    Some((world, x, y, z))
}

/// What a plugin chose for one player's flight and speed; sent whole because the client packet is whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbilityState {
    pub allow_flight: bool,
    pub flying: bool,
    pub invulnerable: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
}
impl Default for AbilityState {
    fn default() -> Self {
        Self {
            allow_flight: false,
            flying: false,
            invulnerable: false,
            fly_speed: 0.1,
            walk_speed: 0.2,
        }
    }
}

/// Applies an ability setter to `state`. `false` means this call is not an ability setter, or its
/// value is outside what Bukkit accepts (speeds are -1..=1), so nothing changed.
pub fn apply_ability(state: &mut AbilityState, call: &ferrite_plugin::HostCall) -> bool {
    let value = call.args.first().map(String::as_str);
    let flag = || match value {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let speed = || {
        value
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|s| s.is_finite() && (-1.0..=1.0).contains(s))
    };
    match (call.owner.as_str(), call.method.as_str()) {
        ("Player", "setFlying(boolean)") => flag().map(|f| state.flying = f).is_some(),
        ("Player", "setAllowFlight(boolean)") => flag().map(|f| state.allow_flight = f).is_some(),
        ("Player", "setInvulnerable(boolean)") => flag().map(|f| state.invulnerable = f).is_some(),
        ("Player", "setFlySpeed(float)") => speed().map(|s| state.fly_speed = s).is_some(),
        ("Player", "setWalkSpeed(float)") => speed().map(|s| state.walk_speed = s).is_some(),
        _ => false,
    }
}

/// The amount of a `Player#damage(...)` call; negative or non-finite amounts are rejected.
pub fn damage_amount(call: &ferrite_plugin::HostCall) -> Option<f32> {
    let named = call.owner == "Player"
        && matches!(
            call.method.as_str(),
            "damage(double)" | "damage(double,Entity)" | "damage(double,DamageSource)"
        );
    named
        .then(|| call.args.first()?.parse::<f32>().ok())
        .flatten()
        .filter(|a| a.is_finite() && *a >= 0.0)
}

/// The game events a plugin call maps to, in the order they must reach the client.
pub fn game_events(call: &ferrite_plugin::HostCall) -> Vec<(u8, f32)> {
    if call.owner != "Player" {
        return Vec::new();
    }
    match (call.method.as_str(), call.args.first().map(String::as_str)) {
        ("showDemoScreen()", _) => vec![(5, 0.0)],
        ("showWinScreen()", _) => vec![(4, 1.0)],
        ("showElderGuardian(boolean)", Some("true" | "false")) => vec![(10, 0.0)],
        // Begin/end raining, then the fade level the client renders.
        ("setPlayerWeather(WeatherType)", Some("DOWNFALL")) => vec![(1, 0.0), (7, 1.0), (8, 0.0)],
        ("setPlayerWeather(WeatherType)", Some("CLEAR")) => vec![(2, 0.0), (7, 0.0), (8, 0.0)],
        _ => Vec::new(),
    }
}

/// What the host does for a generic Bukkit/Paper method call.
#[derive(Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Run a console command, then answer the plugin with the given encoded value.
    Console { line: String, reply: String },
    /// Answer without any side effect.
    Reply(String),
}

/// Maps a [`ferrite_plugin::HostCall`] to its host action, or `None` when the method has no host
/// handler yet (the plugin then gets an `UnsupportedOperationException` naming it).
///
/// Everything that vanilla can already do goes through its command, so the effect is identical to
/// an operator typing it: `Server#broadcastMessage` is `/tellraw @a`, `Player#clearActivePotionEffects`
/// is `/effect clear <uuid>`, and so on.
pub fn host_action(call: &ferrite_plugin::HostCall) -> Option<HostAction> {
    use ferrite_plugin::HOST_NULL;
    let arg = |index: usize| {
        call.args
            .get(index)
            .map(String::as_str)
            .filter(|a| *a != HOST_NULL)
    };
    let console = |line: String, reply: &str| {
        Some(HostAction::Console {
            line,
            reply: reply.to_owned(),
        })
    };
    let player = uuid::Uuid::parse_str(&call.subject).ok();
    match (call.owner.as_str(), call.method.as_str()) {
        ("Server", "broadcastMessage(String)") => {
            let text = arg(0)?;
            console(
                format!("tellraw @a {}", serde_json::json!({ "text": text })),
                "0",
            )
        }
        ("Server", "savePlayers()") => console("save-all".into(), ""),
        ("Server", "shutdown()") => console("stop".into(), ""),
        ("Server", "reloadWhitelist()") => console("whitelist reload".into(), ""),
        ("Server", "setDefaultGameMode(GameMode)") => {
            let mode = arg(0)?.to_ascii_lowercase();
            matches!(
                mode.as_str(),
                "survival" | "creative" | "adventure" | "spectator"
            )
            .then(|| HostAction::Console {
                line: format!("defaultgamemode {mode}"),
                reply: String::new(),
            })
        }
        ("Player", "clearActivePotionEffects()") => {
            console(format!("effect clear {}", player?), "")
        }
        ("Player", "stopAllSounds()") => console(format!("stopsound {} *", player?), ""),
        ("Player", "addPotionEffect(PotionEffect)") => {
            let effect = arg(0)?;
            let mut parts = effect.split(ferrite_plugin::HOST_ITEM_SEPARATOR);
            let key = parts.next().filter(|key| is_resource_key(key))?;
            let ticks: i64 = parts.next()?.parse().ok()?;
            let amplifier: u32 = parts.next()?.parse().ok().filter(|a| *a <= 255)?;
            let _ambient = parts.next()?;
            let particles = parts.next()? == "true";
            let hide = if particles { "false" } else { "true" };
            let line = if ticks < 0 {
                format!("effect give {} {key} infinite {amplifier} {hide}", player?)
            } else {
                let seconds = ((ticks + 19) / 20).clamp(1, 1_000_000);
                format!("effect give {} {key} {seconds} {amplifier} {hide}", player?)
            };
            console(line, "true")
        }
        ("Player", "removePotionEffect(PotionEffectType)") => {
            let key = arg(0).filter(|key| is_resource_key(key))?;
            console(format!("effect clear {} {key}", player?), "")
        }
        (
            "Player",
            "setBedSpawnLocation(Location,boolean)" | "setRespawnLocation(Location,boolean)",
        ) => {
            let (world, x, y, z) = location_block(arg(0)?)?;
            (world == ferrite_plugin::default_world_id())
                .then(|| HostAction::Console {
                    line: format!("spawnpoint {} {x} {y} {z}", player.unwrap_or_default()),
                    reply: String::new(),
                })
                .filter(|_| player.is_some())
        }
        ("Player", "saveData()") => console("save-all".into(), ""),
        ("Server", "reload()" | "reloadData()") => console("reload".into(), ""),
        ("World", "setStorm(boolean)") => {
            default_world(&call.subject)?;
            let rain = arg(0)? == "true";
            console(
                format!("weather {}", if rain { "rain" } else { "clear" }),
                "",
            )
        }
        ("World", "setThundering(boolean)") => {
            default_world(&call.subject)?;
            let thunder = arg(0)? == "true";
            console(
                format!("weather {}", if thunder { "thunder" } else { "clear" }),
                "",
            )
        }
        ("World", "setTime(long)") => {
            default_world(&call.subject)?;
            let time: i64 = arg(0)?.parse().ok()?;
            console(format!("time set {}", time.rem_euclid(24_000)), "")
        }
        ("World", "setDifficulty(Difficulty)") => {
            default_world(&call.subject)?;
            let difficulty = arg(0)?.to_ascii_lowercase();
            matches!(difficulty.as_str(), "peaceful" | "easy" | "normal" | "hard").then(|| {
                HostAction::Console {
                    line: format!("difficulty {difficulty}"),
                    reply: String::new(),
                }
            })
        }
        ("World", "setSpawnLocation(int,int,int)") => {
            default_world(&call.subject)?;
            let (x, y, z) = (
                arg(0)?.parse::<i32>().ok()?,
                arg(1)?.parse::<i32>().ok()?,
                arg(2)?.parse::<i32>().ok()?,
            );
            console(format!("setworldspawn {x} {y} {z}"), "true")
        }
        ("World", "save()") => {
            default_world(&call.subject)?;
            console("save-all".into(), "")
        }
        _ => extra::extra_action(call),
    }
}

/// The vanilla command that performs a plugin's server mutation.
///
/// `/ban-ip` has no expiry or source, so a temporary Bukkit ban is stored as a permanent one;
/// the reason keeps its text on one line because the command reads it as a single argument.
pub fn server_console_line(command: &ferrite_plugin::ServerCommand) -> String {
    match command {
        ferrite_plugin::ServerCommand::BanIp {
            address, reason, ..
        } => match reason
            .as_deref()
            .map(|reason| reason.replace(['\r', '\n'], " "))
        {
            Some(reason) if !reason.trim().is_empty() => format!("ban-ip {address} {reason}"),
            _ => format!("ban-ip {address}"),
        },
        ferrite_plugin::ServerCommand::PardonIp { address } => format!("pardon-ip {address}"),
        ferrite_plugin::ServerCommand::SetWhitelist { enabled } => {
            format!("whitelist {}", if *enabled { "on" } else { "off" })
        }
    }
}

/// Runs plugin-issued console commands on the game loop until either side closes.
pub fn forward_console(
    mut lines: tokio::sync::mpsc::UnboundedReceiver<String>,
    game: tokio::sync::mpsc::UnboundedSender<ferrite_net::game::GameMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(command) = lines.recv().await {
            let (complete, done) = tokio::sync::oneshot::channel();
            let message = ferrite_net::game::GameMessage::Console {
                command: command.clone(),
                complete,
            };
            if game.send(message).is_err() {
                break;
            }
            match done.await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%command, %error, "plugin command failed"),
                Err(_) => break,
            }
        }
    })
}

fn has_plugin_jars(directory: &Path) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_jar_files_count_as_plugins() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_plugin_jars(dir.path()));
        std::fs::write(dir.path().join("readme.txt"), "not a plugin").unwrap();
        assert!(!has_plugin_jars(dir.path()));
        std::fs::write(dir.path().join("empty.jar"), "jar").unwrap();
        assert!(has_plugin_jars(dir.path()));
        assert!(!has_plugin_jars(&dir.path().join("missing")));
    }

    #[test]
    fn host_calls_map_to_vanilla_commands() {
        let call =
            |owner: &str, subject: &str, method: &str, args: &[&str]| ferrite_plugin::HostCall {
                owner: owner.into(),
                subject: subject.into(),
                method: method.into(),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
            };
        let id = "123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(
            host_action(&call(
                "Server",
                "server",
                "broadcastMessage(String)",
                &["hi \"there\""]
            )),
            Some(HostAction::Console {
                line: "tellraw @a {\"text\":\"hi \\\"there\\\"\"}".into(),
                reply: "0".into()
            })
        );
        assert_eq!(
            host_action(&call("Player", id, "clearActivePotionEffects()", &[])),
            Some(HostAction::Console {
                line: format!("effect clear {id}"),
                reply: String::new()
            })
        );
        assert_eq!(
            host_action(&call(
                "Server",
                "server",
                "setDefaultGameMode(GameMode)",
                &["CREATIVE"]
            )),
            Some(HostAction::Console {
                line: "defaultgamemode creative".into(),
                reply: String::new()
            })
        );
        // Bad input and unknown methods are refused instead of guessed.
        assert_eq!(
            host_action(&call(
                "Server",
                "server",
                "setDefaultGameMode(GameMode)",
                &["LOUD"]
            )),
            None
        );
        assert_eq!(
            host_action(&call("Player", "not-a-uuid", "stopAllSounds()", &[])),
            None
        );
        assert_eq!(
            host_action(&call("Player", id, "noSuchMethod()", &[])),
            None
        );
    }

    #[test]
    fn host_calls_cover_effects_damage_world_state_and_spawn() {
        let call =
            |owner: &str, subject: &str, method: &str, args: &[&str]| ferrite_plugin::HostCall {
                owner: owner.into(),
                subject: subject.into(),
                method: method.into(),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
            };
        let line = |call: ferrite_plugin::HostCall| match host_action(&call) {
            Some(HostAction::Console { line, .. }) => Some(line),
            _ => None,
        };
        let id = "123e4567-e89b-12d3-a456-426614174000";
        let world = ferrite_plugin::default_world_id().to_string();
        let sep = ferrite_plugin::HOST_ITEM_SEPARATOR;
        let effect = format!("minecraft:speed{sep}205{sep}1{sep}false{sep}true{sep}true");
        assert_eq!(
            line(call(
                "Player",
                id,
                "addPotionEffect(PotionEffect)",
                &[&effect]
            )),
            Some(format!("effect give {id} minecraft:speed 11 1 false"))
        );
        let forever = format!("minecraft:glowing{sep}-1{sep}0{sep}false{sep}false{sep}true");
        assert_eq!(
            line(call(
                "Player",
                id,
                "addPotionEffect(PotionEffect)",
                &[&forever]
            )),
            Some(format!(
                "effect give {id} minecraft:glowing infinite 0 true"
            ))
        );
        assert_eq!(
            line(call(
                "Player",
                id,
                "removePotionEffect(PotionEffectType)",
                &["minecraft:speed"]
            )),
            Some(format!("effect clear {id} minecraft:speed"))
        );
        // A key that could smuggle a second argument is refused.
        assert_eq!(
            line(call(
                "Player",
                id,
                "removePotionEffect(PotionEffectType)",
                &["speed @a"]
            )),
            None
        );
        assert_eq!(
            damage_amount(&call("Player", id, "damage(double)", &["4.5"])),
            Some(4.5)
        );
        assert_eq!(
            damage_amount(&call("Player", id, "damage(double)", &["-1"])),
            None
        );
        assert_eq!(
            damage_amount(&call("Player", id, "damage(double,Entity)", &["3", id])),
            Some(3.0)
        );
        let bed = format!("world{sep}10.7{sep}64{sep}-3.2{sep}0{sep}0");
        assert_eq!(
            line(call(
                "Player",
                id,
                "setBedSpawnLocation(Location,boolean)",
                &[&bed, "true"]
            )),
            Some(format!("spawnpoint {id} 10 64 -4"))
        );
        assert_eq!(
            line(call(
                "Player",
                id,
                "setRespawnLocation(Location,boolean)",
                &[&bed, "true"]
            )),
            Some(format!("spawnpoint {id} 10 64 -4"))
        );
        assert_eq!(
            line(call("Player", id, "saveData()", &[])),
            Some("save-all".into())
        );
        assert_eq!(
            line(call("World", &world, "setStorm(boolean)", &["true"])),
            Some("weather rain".into())
        );
        assert_eq!(
            line(call("World", &world, "setTime(long)", &["25000"])),
            Some("time set 1000".into())
        );
        assert_eq!(
            line(call(
                "World",
                &world,
                "setDifficulty(Difficulty)",
                &["HARD"]
            )),
            Some("difficulty hard".into())
        );
        assert_eq!(
            line(call(
                "World",
                &world,
                "setSpawnLocation(int,int,int)",
                &["1", "70", "2"]
            )),
            Some("setworldspawn 1 70 2".into())
        );
        // Another world has no console target.
        assert_eq!(
            line(call("World", id, "setStorm(boolean)", &["true"])),
            None
        );
        assert_eq!(
            line(call("Server", "server", "reloadData()", &[])),
            Some("reload".into())
        );
    }

    #[test]
    fn game_events_cover_screens_and_player_weather() {
        let call = |m: &str, a: &str| ferrite_plugin::HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000001".into(),
            method: m.into(),
            args: vec![a.into()],
        };
        assert_eq!(game_events(&call("showDemoScreen()", "")), vec![(5, 0.0)]);
        assert_eq!(
            game_events(&call("setPlayerWeather(WeatherType)", "DOWNFALL")),
            vec![(1, 0.0), (7, 1.0), (8, 0.0)]
        );
        assert!(game_events(&call("setPlayerWeather(WeatherType)", "x")).is_empty());
    }

    #[test]
    fn ability_setters_accumulate_and_reject_bad_values() {
        let call = |method: &str, arg: &str| ferrite_plugin::HostCall {
            owner: "Player".into(),
            subject: "123e4567-e89b-12d3-a456-426614174000".into(),
            method: method.into(),
            args: vec![arg.into()],
        };
        let mut state = AbilityState::default();
        assert!(apply_ability(
            &mut state,
            &call("setAllowFlight(boolean)", "true")
        ));
        assert!(apply_ability(
            &mut state,
            &call("setFlying(boolean)", "true")
        ));
        assert!(apply_ability(
            &mut state,
            &call("setFlySpeed(float)", "0.5")
        ));
        assert_eq!(
            state,
            AbilityState {
                allow_flight: true,
                flying: true,
                invulnerable: false,
                fly_speed: 0.5,
                walk_speed: 0.2
            }
        );
        let before = state;
        assert!(!apply_ability(
            &mut state,
            &call("setWalkSpeed(float)", "2.0")
        ));
        assert!(!apply_ability(
            &mut state,
            &call("setWalkSpeed(float)", "NaN")
        ));
        assert!(!apply_ability(
            &mut state,
            &call("setFlying(boolean)", "maybe")
        ));
        assert!(!apply_ability(&mut state, &call("getFlying()", "")));
        assert_eq!(state, before);
    }

    #[test]
    fn plugin_world_and_server_mutations_become_console_commands() {
        use ferrite_plugin::{BlockPosition, ServerCommand, WorldCommand};
        let set = |world| WorldCommand::SetBlock {
            world,
            position: BlockPosition { x: 1, y: -2, z: 3 },
            material: "minecraft:stone".into(),
        };
        assert_eq!(
            world_console_line(&set(ferrite_plugin::default_world_id())).as_deref(),
            Some("setblock 1 -2 3 minecraft:stone")
        );
        assert_eq!(world_console_line(&set(uuid::Uuid::nil())), None);

        let address = "10.0.0.1".parse().unwrap();
        let ban = |reason: Option<&str>| ServerCommand::BanIp {
            address,
            reason: reason.map(str::to_owned),
            expires_unix_millis: Some(1),
            source: Some("Plugin".into()),
        };
        assert_eq!(
            server_console_line(&ban(Some("spam\nbot"))),
            "ban-ip 10.0.0.1 spam bot"
        );
        assert_eq!(server_console_line(&ban(None)), "ban-ip 10.0.0.1");
        assert_eq!(server_console_line(&ban(Some(" "))), "ban-ip 10.0.0.1");
        assert_eq!(
            server_console_line(&ServerCommand::PardonIp { address }),
            "pardon-ip 10.0.0.1"
        );
        assert_eq!(
            server_console_line(&ServerCommand::SetWhitelist { enabled: true }),
            "whitelist on"
        );
        assert_eq!(
            server_console_line(&ServerCommand::SetWhitelist { enabled: false }),
            "whitelist off"
        );
    }
}
