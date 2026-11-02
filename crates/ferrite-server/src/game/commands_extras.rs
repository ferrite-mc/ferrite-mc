//! Player-feedback commands: `/effect`, `/enchant`, `/title`, `/tellraw`, `/playsound`,
//! `/stopsound`, `/particle`, `/spawnpoint`, `/setworldspawn` and `/worldborder`.
//!
//! Parsing, permissions and state changes happen here on the command thread. Packets are sent
//! through [`Action::Packets`] and applied by the game loop, so every player still receives
//! them from the loop that owns the connection.

use ferrite_command::{
    ArgumentType, ArgumentValue, CommandContext, CommandDispatcher, CommandError, NodeBuilder,
    argument,
    arguments::{RegistryParser, coords::resolve_rotation},
    literal,
};
use ferrite_net::game::{OutboundSender, packet};
use ferrite_protocol::{
    VarLong,
    packets::play::{
        BossBar, ClearTitles, DefaultSpawn, LevelParticles, PlayerTeam, ResetScore,
        SetActionBarText, SetBorderCenter, SetBorderLerpSize, SetBorderSize, SetBorderWarningDelay,
        SetBorderWarningDistance, SetDisplayObjective, SetObjective, SetScore, SetSubtitleText,
        SetTitleText, SetTitlesAnimation, SoundEffect, StopSound, SystemChat,
    },
    types::{BlockPos, PlainText},
};
use uuid::Uuid;

use super::commands::{Action, Feedback, Source, coordinates, resolved_targets, send};

/// Vanilla's `Overworld` dimension key, the only dimension the server currently has.
const OVERWORLD: &str = "minecraft:overworld";
/// The largest world border diameter vanilla accepts.
const MAX_BORDER: f64 = 59_999_968.0;

/// A packet that a command wants sent to one player, several players or everybody.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerPacket {
    Title(SetTitleText),
    Subtitle(SetSubtitleText),
    ActionBar(SetActionBarText),
    TitleTimes(SetTitlesAnimation),
    ClearTitles(ClearTitles),
    Tellraw(SystemChat),
    Sound(SoundEffect),
    StopSound(StopSound),
    Particles(LevelParticles),
    BorderSize(SetBorderSize),
    BorderLerp(SetBorderLerpSize),
    BorderCenter(SetBorderCenter),
    BorderWarningDelay(SetBorderWarningDelay),
    BorderWarningDistance(SetBorderWarningDistance),
    DefaultSpawn(DefaultSpawn),
    SetObjective(SetObjective),
    SetScore(SetScore),
    ResetScore(ResetScore),
    SetDisplayObjective(SetDisplayObjective),
    BossBar(BossBar),
    PlayerTeam(PlayerTeam),
}

impl PlayerPacket {
    /// Encodes and queues the packet on a connection.
    pub fn send_to(&self, outbound: &OutboundSender) {
        match self {
            Self::Title(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::Subtitle(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::ActionBar(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::TitleTimes(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::ClearTitles(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::Tellraw(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::Sound(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::StopSound(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::Particles(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BorderSize(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BorderLerp(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BorderCenter(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BorderWarningDelay(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BorderWarningDistance(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::DefaultSpawn(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::SetObjective(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::SetScore(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::ResetScore(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::SetDisplayObjective(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::BossBar(p) => {
                let _ = outbound.send(packet(p));
            }
            Self::PlayerTeam(p) => {
                let _ = outbound.send(packet(p));
            }
        }
    }
}

/// Registers each command once. `/effect`, `/enchant`, `/title`, `/tellraw`, `/playsound`,
/// `/stopsound`, `/particle`, `/spawnpoint`, `/setworldspawn` and `/worldborder` are gated at
/// gamemasters, matching the 26.3 `commands.json` report.
pub fn register(tree: &mut CommandDispatcher<Source>) {
    tree.register(effect_tree());
    tree.register(enchant_tree());
    tree.register(title_tree());
    tree.register(tellraw_tree());
    tree.register(playsound_tree());
    tree.register(stopsound_tree());
    tree.register(particle_tree());
    tree.register(spawnpoint_tree());
    tree.register(setworldspawn_tree());
    tree.register(worldborder_tree());
}

fn gamemasters() -> impl Fn(&Source) -> bool + Send + Sync + 'static {
    |source| source.permission >= 2
}

pub(crate) fn packets(
    ctx: &CommandContext<Source>,
    targets: Vec<Uuid>,
    packet: PlayerPacket,
) -> Result<i32, CommandError> {
    send(ctx.source(), Action::Packets { targets, packet })
}

pub(crate) fn feedback(
    ctx: &CommandContext<Source>,
    key: &'static str,
    args: Vec<String>,
) -> Result<i32, CommandError> {
    send(ctx.source(), Action::Feedback(Feedback::new(key, args)))
}

fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

fn title_words(path: &str) -> String {
    path.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn registry_id(registry: &str, raw: &str) -> Result<i32, CommandError> {
    let id = namespaced(raw);
    ferrite_data::registries::id(registry, &id).ok_or_else(|| {
        CommandError::failed(format!("Can't find element '{id}' of type '{registry}'"))
    })
}

fn effect_id(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let raw = ctx.string("effect").unwrap_or_default();
    registry_id("minecraft:mob_effect", raw)
}

fn effect_display(effect: i32) -> String {
    let name = ferrite_data::registries::entries("minecraft:mob_effect")
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| {
                    ferrite_data::registries::id("minecraft:mob_effect", entry) == Some(effect)
                })
                .copied()
        })
        .unwrap_or("unknown");
    title_words(name.strip_prefix("minecraft:").unwrap_or(name))
}

fn target_names(ctx: &CommandContext<Source>, targets: &[Uuid]) -> Vec<String> {
    let state = ctx.source().state.lock().unwrap();
    targets
        .iter()
        .map(|uuid| {
            state
                .players
                .iter()
                .find(|player| player.uuid == *uuid)
                .map(|player| player.name.clone())
                .unwrap_or_else(|| uuid.to_string())
        })
        .collect()
}

fn feedback_for_targets(
    ctx: &CommandContext<Source>,
    single: &'static str,
    multiple: &'static str,
    targets: &[Uuid],
) -> Result<i32, CommandError> {
    if targets.len() == 1 {
        feedback(ctx, single, target_names(ctx, targets))
    } else {
        feedback(ctx, multiple, vec![targets.len().to_string()])
    }
}

fn vec3(ctx: &CommandContext<Source>, name: &str) -> Result<[f64; 3], CommandError> {
    let coordinates = coordinates(ctx, name)?;
    Ok(coordinates.resolve(ctx.source().origin, ctx.source().yaw, ctx.source().pitch))
}

fn block_pos(ctx: &CommandContext<Source>, name: &str) -> Result<[i32; 3], CommandError> {
    let position = vec3(ctx, name)?;
    Ok([
        position[0].floor() as i32,
        position[1].floor() as i32,
        position[2].floor() as i32,
    ])
}

fn rotation(ctx: &CommandContext<Source>, name: &str) -> Result<(f32, f32), CommandError> {
    match ctx.argument(name) {
        Some(ArgumentValue::Rotation(rotation)) => Ok(resolve_rotation(
            *rotation,
            ctx.source().yaw,
            ctx.source().pitch,
        )),
        _ => Err(CommandError::failed(format!("missing {name}"))),
    }
}

pub(crate) fn component_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{')
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        return json_component_text(&value);
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

fn json_component_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(values) => values.iter().map(json_component_text).collect(),
        serde_json::Value::Object(fields) => {
            let mut text = fields
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if text.is_empty()
                && let Some(key) = fields.get("translate").and_then(serde_json::Value::as_str)
            {
                let args = fields
                    .get("with")
                    .and_then(serde_json::Value::as_array)
                    .map(|values| values.iter().map(json_component_text).collect::<Vec<_>>())
                    .unwrap_or_default();
                let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                text = super::feedback::render(key, &args);
            }
            if let Some(extra) = fields.get("extra").and_then(serde_json::Value::as_array) {
                for part in extra {
                    text.push_str(&json_component_text(part));
                }
            }
            text
        }
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// /effect
// ---------------------------------------------------------------------------------------------

fn effect_tree() -> NodeBuilder<Source> {
    let effect = || ArgumentType::Registry {
        parser: RegistryParser::Resource,
        registry: "minecraft:mob_effect".into(),
    };
    literal::<Source>("effect")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("clear").executes(effect_clear_self).then(
                argument("targets", ArgumentType::entities())
                    .executes(effect_clear_targets)
                    .then(argument("effect", effect()).executes(effect_clear_specific)),
            ),
        )
        .then(
            literal("give").then(
                argument("targets", ArgumentType::entities()).then(
                    argument("effect", effect())
                        .then(
                            literal("infinite").executes(effect_give_infinite).then(
                                argument("amplifier", ArgumentType::integer_range(0, 255))
                                    .executes(effect_give_infinite_amplifier),
                            ),
                        )
                        .then(
                            argument("seconds", ArgumentType::integer_range(1, 1_000_000))
                                .executes(effect_give_seconds)
                                .then(
                                    argument("amplifier", ArgumentType::integer_range(0, 255))
                                        .executes(effect_give_seconds_amplifier),
                                ),
                        ),
                ),
            ),
        )
}

fn effect_clear_self(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_clear(ctx, false)
}

fn effect_clear_targets(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_clear(ctx, false)
}

fn effect_clear_specific(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_clear(ctx, true)
}

fn effect_give_infinite(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_give(ctx, -1)
}

fn effect_give_infinite_amplifier(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_give(ctx, -1)
}

fn effect_give_seconds(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let seconds = ctx.integer("seconds").unwrap_or(0);
    effect_give(ctx, seconds.saturating_mul(20))
}

fn effect_give_seconds_amplifier(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    effect_give_seconds(ctx)
}

fn effect_give(ctx: &CommandContext<Source>, duration: i32) -> Result<i32, CommandError> {
    let targets = if ctx.argument("targets").is_some() {
        resolved_targets(ctx, "targets")?
    } else {
        vec![ctx.source().uuid.ok_or_else(super::commands::player_only)?]
    };
    let effect = effect_id(ctx)?;
    let amplifier = ctx.integer("amplifier").unwrap_or(0);
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            if let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *target)
            {
                player.effects.insert(effect, amplifier);
            }
        }
    }
    send(
        ctx.source(),
        Action::Effect {
            targets: targets.clone(),
            effect,
            amplifier,
            duration,
        },
    )?;
    let effect = effect_display(effect);
    if targets.len() == 1 {
        let target = target_names(ctx, &targets).remove(0);
        feedback(
            ctx,
            "commands.effect.give.success.single",
            vec![effect, target],
        )
    } else {
        feedback(
            ctx,
            "commands.effect.give.success.multiple",
            vec![effect, targets.len().to_string()],
        )
    }
}

fn effect_clear(ctx: &CommandContext<Source>, specific: bool) -> Result<i32, CommandError> {
    let targets = if ctx.argument("targets").is_some() {
        resolved_targets(ctx, "targets")?
    } else {
        vec![ctx.source().uuid.ok_or_else(super::commands::player_only)?]
    };
    let effect = if specific {
        Some(effect_id(ctx)?)
    } else {
        None
    };
    let mut removed = Vec::new();
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *target)
            else {
                continue;
            };
            match effect {
                Some(effect) => {
                    if player.effects.remove(&effect).is_some() {
                        removed.push((*target, effect));
                    }
                }
                None => {
                    for effect in std::mem::take(&mut player.effects).into_keys() {
                        removed.push((*target, effect));
                    }
                }
            }
        }
    }
    if removed.is_empty() {
        return Err(CommandError::failed(if specific {
            "Target doesn't have the requested effect"
        } else {
            "Target has no effects to remove"
        }));
    }
    for (target, effect) in removed {
        send(
            ctx.source(),
            Action::RemoveEffect {
                targets: vec![target],
                effect,
            },
        )?;
    }
    let effect = effect.map(effect_display);
    if targets.len() == 1 {
        let target = target_names(ctx, &targets).remove(0);
        match effect {
            Some(effect) => feedback(
                ctx,
                "commands.effect.clear.specific.success.single",
                vec![effect, target],
            ),
            None => feedback(
                ctx,
                "commands.effect.clear.everything.success.single",
                vec![target],
            ),
        }
    } else {
        let count = targets.len().to_string();
        match effect {
            Some(effect) => feedback(
                ctx,
                "commands.effect.clear.specific.success.multiple",
                vec![effect, count],
            ),
            None => feedback(
                ctx,
                "commands.effect.clear.everything.success.multiple",
                vec![count],
            ),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// /enchant
// ---------------------------------------------------------------------------------------------

fn enchant_tree() -> NodeBuilder<Source> {
    literal::<Source>("enchant")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument("targets", ArgumentType::entities()).then(
                argument(
                    "enchantment",
                    ArgumentType::Registry {
                        parser: RegistryParser::Resource,
                        registry: "minecraft:enchantment".into(),
                    },
                )
                .executes(enchant_item)
                .then(
                    argument("level", ArgumentType::integer_range(0, i32::MAX))
                        .executes(enchant_item),
                ),
            ),
        )
}

fn enchant_item(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    let raw = ctx.string("enchantment").unwrap_or_default();
    let enchantment = namespaced(raw);
    if let Some(entries) = ferrite_data::registries::data_entries("enchantment")
        && !entries.iter().any(|entry| *entry == enchantment)
    {
        return Err(CommandError::failed(format!(
            "Can't find element '{enchantment}' of type 'minecraft:enchantment'"
        )));
    }
    let level = ctx
        .integer("level")
        .ok_or_else(|| CommandError::failed("Missing enchantment level"))?;
    send(
        ctx.source(),
        Action::Enchant {
            targets,
            enchantment,
            level,
        },
    )
}

// ---------------------------------------------------------------------------------------------
// /title
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum TitlePart {
    Title,
    Subtitle,
    ActionBar,
}

fn title_tree() -> NodeBuilder<Source> {
    literal::<Source>("title")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument("targets", ArgumentType::players())
                .then(
                    literal("actionbar")
                        .then(argument("title", ArgumentType::Component).executes(title_actionbar)),
                )
                .then(literal("clear").executes(title_clear))
                .then(literal("reset").executes(title_reset))
                .then(
                    literal("subtitle")
                        .then(argument("title", ArgumentType::Component).executes(title_subtitle)),
                )
                .then(
                    literal("times").then(
                        argument("fadeIn", ArgumentType::Time { min: 0 }).then(
                            argument("stay", ArgumentType::Time { min: 0 }).then(
                                argument("fadeOut", ArgumentType::Time { min: 0 })
                                    .executes(title_times),
                            ),
                        ),
                    ),
                )
                .then(
                    literal("title")
                        .then(argument("title", ArgumentType::Component).executes(title_main)),
                ),
        )
}

fn title_main(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    title_show(ctx, TitlePart::Title)
}

fn title_subtitle(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    title_show(ctx, TitlePart::Subtitle)
}

fn title_actionbar(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    title_show(ctx, TitlePart::ActionBar)
}

fn title_show(ctx: &CommandContext<Source>, part: TitlePart) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    let text = PlainText(component_text(ctx.string("title").unwrap_or_default()));
    let packet = match part {
        TitlePart::Title => PlayerPacket::Title(SetTitleText { text }),
        TitlePart::Subtitle => PlayerPacket::Subtitle(SetSubtitleText { text }),
        TitlePart::ActionBar => PlayerPacket::ActionBar(SetActionBarText { text }),
    };
    packets(ctx, targets.clone(), packet)?;
    let (single, multiple) = match part {
        TitlePart::Title => (
            "commands.title.show.title.single",
            "commands.title.show.title.multiple",
        ),
        TitlePart::Subtitle => (
            "commands.title.show.subtitle.single",
            "commands.title.show.subtitle.multiple",
        ),
        TitlePart::ActionBar => (
            "commands.title.show.actionbar.single",
            "commands.title.show.actionbar.multiple",
        ),
    };
    feedback_for_targets(ctx, single, multiple, &targets)
}

fn title_times(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    let fade_in = ctx.integer("fadeIn").unwrap_or(0);
    let stay = ctx.integer("stay").unwrap_or(0);
    let fade_out = ctx.integer("fadeOut").unwrap_or(0);
    packets(
        ctx,
        targets.clone(),
        PlayerPacket::TitleTimes(SetTitlesAnimation {
            fade_in,
            stay,
            fade_out,
        }),
    )?;
    feedback_for_targets(
        ctx,
        "commands.title.times.single",
        "commands.title.times.multiple",
        &targets,
    )
}

fn title_clear(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    packets(
        ctx,
        targets.clone(),
        PlayerPacket::ClearTitles(ClearTitles { reset: false }),
    )?;
    feedback_for_targets(
        ctx,
        "commands.title.cleared.single",
        "commands.title.cleared.multiple",
        &targets,
    )
}

fn title_reset(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    packets(
        ctx,
        targets.clone(),
        PlayerPacket::ClearTitles(ClearTitles { reset: true }),
    )?;
    feedback_for_targets(
        ctx,
        "commands.title.reset.single",
        "commands.title.reset.multiple",
        &targets,
    )
}

// ---------------------------------------------------------------------------------------------
// /tellraw
// ---------------------------------------------------------------------------------------------

fn tellraw_tree() -> NodeBuilder<Source> {
    literal::<Source>("tellraw")
        .requires(gamemasters())
        .restricted(true)
        .then(argument("targets", ArgumentType::players()).then(
            argument("message", ArgumentType::Component).executes(|ctx| {
                let targets = resolved_targets(ctx, "targets")?;
                let text = component_text(ctx.string("message").unwrap_or_default());
                packets(
                    ctx,
                    targets,
                    PlayerPacket::Tellraw(SystemChat {
                        content: PlainText(text),
                        overlay: false,
                    }),
                )
            }),
        ))
}

// ---------------------------------------------------------------------------------------------
// /playsound
// ---------------------------------------------------------------------------------------------

const SOUND_SOURCES: [(&str, i32); 11] = [
    ("master", 0),
    ("music", 1),
    ("record", 2),
    ("weather", 3),
    ("block", 4),
    ("hostile", 5),
    ("neutral", 6),
    ("player", 7),
    ("ambient", 8),
    ("voice", 9),
    ("ui", 10),
];

fn sound_id(raw: &str) -> Result<i32, CommandError> {
    let id = namespaced(raw);
    ferrite_data::registries::id("minecraft:sound_event", &id).ok_or_else(|| {
        CommandError::failed(format!(
            "Can't find element '{id}' of type 'minecraft:sound_event'"
        ))
    })
}

fn sound_source_branch(name: &'static str, source: i32) -> NodeBuilder<Source> {
    literal::<Source>(name).then(
        argument("targets", ArgumentType::players())
            .executes(move |ctx| play_sound(ctx, source, false))
            .then(
                argument("pos", ArgumentType::Vec3)
                    .executes(move |ctx| play_sound(ctx, source, true))
                    .then(
                        argument("volume", ArgumentType::float())
                            .executes(move |ctx| play_sound(ctx, source, true))
                            .then(
                                argument("pitch", ArgumentType::float())
                                    .executes(move |ctx| play_sound(ctx, source, true))
                                    .then(
                                        argument("minVolume", ArgumentType::float())
                                            .executes(move |ctx| play_sound(ctx, source, true)),
                                    ),
                            ),
                    ),
            ),
    )
}

fn playsound_tree() -> NodeBuilder<Source> {
    let mut sound = argument("sound", ArgumentType::ResourceLocation);
    for (name, id) in SOUND_SOURCES {
        sound = sound.then(sound_source_branch(name, id));
    }
    literal::<Source>("playsound")
        .requires(gamemasters())
        .restricted(true)
        .then(sound)
}

fn play_sound(
    ctx: &CommandContext<Source>,
    source: i32,
    positional: bool,
) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    let raw = ctx.string("sound").unwrap_or_default().to_owned();
    let id = sound_id(&raw)?;
    let position = if positional {
        vec3(ctx, "pos")?
    } else {
        ctx.source().origin
    };
    let volume = ctx
        .argument("volume")
        .and_then(ArgumentValue::as_f32)
        .unwrap_or(1.0);
    if volume < 0.0 {
        return Err(CommandError::failed(format!(
            "Float must not be less than 0.0: found {}",
            ferrite_command::error::java_float(volume)
        )));
    }
    let pitch = ctx
        .argument("pitch")
        .and_then(ArgumentValue::as_f32)
        .unwrap_or(1.0);
    if !(0.5..=2.0).contains(&pitch) {
        return Err(CommandError::failed(format!(
            "Pitch must be between 0.5 and 2.0: found {}",
            ferrite_command::error::java_float(pitch)
        )));
    }
    packets(
        ctx,
        targets.clone(),
        PlayerPacket::Sound(SoundEffect {
            sound: ferrite_protocol::VarInt(id),
            source: ferrite_protocol::VarInt(source),
            x: (position[0] * 8.0) as i32,
            y: (position[1] * 8.0) as i32,
            z: (position[2] * 8.0) as i32,
            volume,
            pitch,
            seed: 0,
        }),
    )?;
    if targets.len() == 1 {
        let target = target_names(ctx, &targets).remove(0);
        feedback(ctx, "commands.playsound.success.single", vec![raw, target])
    } else {
        feedback(
            ctx,
            "commands.playsound.success.multiple",
            vec![raw, targets.len().to_string()],
        )
    }
}

// ---------------------------------------------------------------------------------------------
// /stopsound
// ---------------------------------------------------------------------------------------------

fn stopsound_tree() -> NodeBuilder<Source> {
    let mut targets = argument("targets", ArgumentType::players())
        .executes(|ctx| stop_sound(ctx, None, false))
        .then(
            literal("*")
                .executes(|ctx| stop_sound(ctx, None, false))
                .then(
                    argument("sound", ArgumentType::ResourceLocation)
                        .executes(|ctx| stop_sound(ctx, None, true)),
                ),
        );
    for (name, source) in SOUND_SOURCES {
        let source = Some(source);
        targets = targets.then(
            literal::<Source>(name)
                .executes(move |ctx| stop_sound(ctx, source, false))
                .then(
                    argument("sound", ArgumentType::ResourceLocation)
                        .executes(move |ctx| stop_sound(ctx, source, true)),
                ),
        );
    }
    literal::<Source>("stopsound")
        .requires(gamemasters())
        .restricted(true)
        .then(targets)
}

fn stop_sound(
    ctx: &CommandContext<Source>,
    source: Option<i32>,
    with_sound: bool,
) -> Result<i32, CommandError> {
    let targets = resolved_targets(ctx, "targets")?;
    let sound = if with_sound {
        let raw = ctx.string("sound").unwrap_or_default().to_owned();
        sound_id(&raw)?;
        Some(raw)
    } else {
        None
    };
    let source_name = source.and_then(|id| {
        SOUND_SOURCES
            .iter()
            .find(|(_, source)| *source == id)
            .map(|(name, _)| *name)
    });
    packets(
        ctx,
        targets.clone(),
        PlayerPacket::StopSound(StopSound {
            source: source.map(ferrite_protocol::VarInt),
            sound: sound.clone(),
        }),
    )?;
    match (source, sound) {
        (Some(_), Some(sound)) => feedback(
            ctx,
            "commands.stopsound.success.source.sound",
            vec![sound, source_name.unwrap_or("master").to_owned()],
        ),
        (Some(_), None) => feedback(
            ctx,
            "commands.stopsound.success.source.any",
            vec![source_name.unwrap_or("master").to_owned()],
        ),
        (None, Some(sound)) => feedback(
            ctx,
            "commands.stopsound.success.sourceless.sound",
            vec![sound],
        ),
        (None, None) => feedback(ctx, "commands.stopsound.success.sourceless.any", Vec::new()),
    }
}

// ---------------------------------------------------------------------------------------------
// /particle
// ---------------------------------------------------------------------------------------------

fn particle_tree() -> NodeBuilder<Source> {
    literal::<Source>("particle")
        .requires(gamemasters())
        .restricted(true)
        .then(
            argument("name", ArgumentType::opaque("minecraft:particle"))
                .executes(|ctx| particle(ctx, false, false, false))
                .then(
                    argument("pos", ArgumentType::Vec3)
                        .executes(|ctx| particle(ctx, true, false, false))
                        .then(
                            argument("delta", ArgumentType::Vec3).then(
                                argument("speed", ArgumentType::float()).then(
                                    argument("count", ArgumentType::integer_range(0, i32::MAX))
                                        .executes(|ctx| particle(ctx, true, true, true)),
                                ),
                            ),
                        ),
                ),
        )
}

fn particle(
    ctx: &CommandContext<Source>,
    positional: bool,
    has_delta: bool,
    full: bool,
) -> Result<i32, CommandError> {
    let raw = ctx.string("name").unwrap_or_default().to_owned();
    let particle = registry_id("minecraft:particle_type", &raw)?;
    let position = if positional {
        vec3(ctx, "pos")?
    } else {
        ctx.source().origin
    };
    let delta = if has_delta {
        vec3(ctx, "delta")?
    } else {
        [0.0; 3]
    };
    let speed = ctx
        .argument("speed")
        .and_then(ArgumentValue::as_f32)
        .unwrap_or(0.0);
    if speed < 0.0 {
        return Err(CommandError::failed(format!(
            "Float must not be less than 0.0: found {}",
            ferrite_command::error::java_float(speed)
        )));
    }
    let count = if full {
        ctx.integer("count").unwrap_or(1)
    } else {
        1
    };
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::Particles(LevelParticles {
            particle: ferrite_protocol::VarInt(particle),
            long_distance: false,
            always_show: true,
            x: position[0],
            y: position[1],
            z: position[2],
            velocity_x: delta[0] as f32,
            velocity_y: delta[1] as f32,
            velocity_z: delta[2] as f32,
            speed,
            count,
        }),
    )?;
    feedback(ctx, "commands.particle.success", vec![raw])
}

// ---------------------------------------------------------------------------------------------
// /spawnpoint and /setworldspawn
// ---------------------------------------------------------------------------------------------

fn spawnpoint_tree() -> NodeBuilder<Source> {
    literal::<Source>("spawnpoint")
        .requires(gamemasters())
        .restricted(true)
        .executes(spawnpoint_self)
        .then(
            argument("targets", ArgumentType::entities())
                .executes(spawnpoint_targets)
                .then(
                    argument("pos", ArgumentType::BlockPos)
                        .executes(spawnpoint_targets_pos)
                        .then(
                            argument("rotation", ArgumentType::Rotation)
                                .executes(spawnpoint_targets_pos_rotation),
                        ),
                ),
        )
}

fn spawnpoint_self(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    spawnpoint(ctx)
}

fn spawnpoint_targets(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    spawnpoint(ctx)
}

fn spawnpoint_targets_pos(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    spawnpoint(ctx)
}

fn spawnpoint_targets_pos_rotation(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    spawnpoint(ctx)
}

fn spawnpoint(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let targets = if ctx.argument("targets").is_some() {
        resolved_targets(ctx, "targets")?
    } else {
        vec![ctx.source().uuid.ok_or_else(super::commands::player_only)?]
    };
    let position = if ctx.argument("pos").is_some() {
        block_pos(ctx, "pos")?
    } else {
        [
            ctx.source().origin[0].floor() as i32,
            ctx.source().origin[1].floor() as i32,
            ctx.source().origin[2].floor() as i32,
        ]
    };
    let (yaw, pitch) = if ctx.argument("rotation").is_some() {
        rotation(ctx, "rotation")?
    } else {
        (0.0, 0.0)
    };
    {
        let mut state = ctx.source().state.lock().unwrap();
        for target in &targets {
            if let Some(player) = state
                .players
                .iter_mut()
                .find(|player| player.uuid == *target)
            {
                player.spawn = Some((position, yaw, pitch));
            }
        }
    }
    let mut args = vec![
        position[0].to_string(),
        position[1].to_string(),
        position[2].to_string(),
        ferrite_command::error::java_float(yaw),
        ferrite_command::error::java_float(pitch),
        OVERWORLD.to_owned(),
    ];
    if targets.len() == 1 {
        args.extend(target_names(ctx, &targets));
        feedback(ctx, "commands.spawnpoint.success.single.new", args)
    } else {
        args.push(targets.len().to_string());
        feedback(ctx, "commands.spawnpoint.success.multiple.new", args)
    }
}

fn setworldspawn_tree() -> NodeBuilder<Source> {
    literal::<Source>("setworldspawn")
        .requires(gamemasters())
        .restricted(true)
        .executes(setworldspawn_default)
        .then(
            argument("pos", ArgumentType::BlockPos)
                .executes(setworldspawn)
                .then(argument("rotation", ArgumentType::Rotation).executes(setworldspawn)),
        )
}

fn setworldspawn_default(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    setworldspawn(ctx)
}

fn setworldspawn(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let position = if ctx.argument("pos").is_some() {
        block_pos(ctx, "pos")?
    } else {
        [
            ctx.source().origin[0].floor() as i32,
            ctx.source().origin[1].floor() as i32,
            ctx.source().origin[2].floor() as i32,
        ]
    };
    let (yaw, pitch) = if ctx.argument("rotation").is_some() {
        rotation(ctx, "rotation")?
    } else {
        (0.0, 0.0)
    };
    {
        let mut state = ctx.source().state.lock().unwrap();
        state.world_spawn = position;
        state.world_spawn_yaw = yaw;
        state.world_spawn_pitch = pitch;
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::DefaultSpawn(DefaultSpawn {
            dimension: OVERWORLD.to_owned(),
            position: BlockPos::new(position[0], position[1], position[2]).pack(),
            yaw,
            pitch,
        }),
    )?;
    feedback(
        ctx,
        "commands.setworldspawn.success.new",
        vec![
            position[0].to_string(),
            position[1].to_string(),
            position[2].to_string(),
            ferrite_command::error::java_float(yaw),
            ferrite_command::error::java_float(pitch),
            OVERWORLD.to_owned(),
        ],
    )
}

// ---------------------------------------------------------------------------------------------
// /worldborder
// ---------------------------------------------------------------------------------------------

fn worldborder_tree() -> NodeBuilder<Source> {
    literal::<Source>("worldborder")
        .requires(gamemasters())
        .restricted(true)
        .then(
            literal("add").then(
                argument("distance", ArgumentType::double())
                    .executes(|ctx| worldborder_resize(ctx, true))
                    .then(
                        argument("time", ArgumentType::Time { min: 0 })
                            .executes(|ctx| worldborder_resize(ctx, true)),
                    ),
            ),
        )
        .then(
            literal("center")
                .then(argument("pos", ArgumentType::Vec2).executes(worldborder_center)),
        )
        .then(
            literal("damage")
                .then(
                    literal("amount").then(
                        argument("damagePerBlock", ArgumentType::float())
                            .executes(worldborder_damage_amount),
                    ),
                )
                .then(literal("buffer").then(
                    argument("distance", ArgumentType::float()).executes(worldborder_damage_buffer),
                )),
        )
        .then(literal("get").executes(worldborder_get))
        .then(
            literal("set").then(
                argument("distance", ArgumentType::double())
                    .executes(|ctx| worldborder_resize(ctx, false))
                    .then(
                        argument("time", ArgumentType::Time { min: 0 })
                            .executes(|ctx| worldborder_resize(ctx, false)),
                    ),
            ),
        )
        .then(
            literal("warning")
                .then(
                    literal("distance").then(
                        argument("distance", ArgumentType::integer_range(0, i32::MAX))
                            .executes(worldborder_warning_distance),
                    ),
                )
                .then(
                    literal("time").then(
                        argument("time", ArgumentType::Time { min: 0 })
                            .executes(worldborder_warning_time),
                    ),
                ),
        )
}

fn worldborder_resize(ctx: &CommandContext<Source>, add: bool) -> Result<i32, CommandError> {
    let distance = ctx
        .argument("distance")
        .and_then(ArgumentValue::as_f64)
        .ok_or_else(|| CommandError::failed("Missing distance"))?;
    let time = ctx.integer("time").unwrap_or(0);
    let mut state = ctx.source().state.lock().unwrap();
    let old = state.border.diameter;
    let new = if add { old + distance } else { distance };
    if new < 1.0 {
        return Err(CommandError::failed(
            "World border cannot be smaller than 1 block wide",
        ));
    }
    if new > MAX_BORDER {
        return Err(CommandError::failed(format!(
            "World border cannot be bigger than {} blocks wide",
            ferrite_command::error::java_double(MAX_BORDER)
        )));
    }
    if (new - old).abs() < f64::EPSILON {
        return Err(CommandError::failed(
            "Nothing changed. The world border is already that size",
        ));
    }
    state.border.diameter = new;
    drop(state);
    if time == 0 {
        packets(
            ctx,
            Vec::new(),
            PlayerPacket::BorderSize(SetBorderSize { diameter: new }),
        )?;
        feedback(
            ctx,
            "commands.worldborder.set.immediate",
            vec![ferrite_command::error::java_double(new)],
        )
    } else {
        let speed = ((new - old).abs() / f64::from(time)).ceil() as i64;
        packets(
            ctx,
            Vec::new(),
            PlayerPacket::BorderLerp(SetBorderLerpSize {
                old_diameter: old,
                new_diameter: new,
                speed: VarLong(speed.max(1)),
            }),
        )?;
        let key = if new > old {
            "commands.worldborder.set.grow"
        } else {
            "commands.worldborder.set.shrink"
        };
        feedback(
            ctx,
            key,
            vec![
                ferrite_command::error::java_double(new),
                ferrite_command::error::java_double(f64::from(time) / 20.0),
            ],
        )
    }
}

fn worldborder_center(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let Some(ArgumentValue::Vec2([x, z])) = ctx.argument("pos") else {
        return Err(CommandError::failed("Missing position"));
    };
    let center = [
        x.resolve(ctx.source().origin[0]),
        z.resolve(ctx.source().origin[2]),
    ];
    {
        let mut state = ctx.source().state.lock().unwrap();
        state.border.center_x = center[0];
        state.border.center_z = center[1];
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BorderCenter(SetBorderCenter {
            x: center[0],
            z: center[1],
        }),
    )?;
    feedback(
        ctx,
        "commands.worldborder.center.success",
        vec![
            ferrite_command::error::java_double(center[0]),
            ferrite_command::error::java_double(center[1]),
        ],
    )
}

fn worldborder_get(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let diameter = ctx.source().state.lock().unwrap().border.diameter;
    feedback(
        ctx,
        "commands.worldborder.get",
        vec![ferrite_command::error::java_double(diameter)],
    )
}

fn worldborder_damage_amount(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let amount = ctx
        .argument("damagePerBlock")
        .and_then(ArgumentValue::as_f32)
        .ok_or_else(|| CommandError::failed("Missing damage amount"))?;
    if amount < 0.0 {
        return Err(CommandError::failed(
            "Float must not be less than 0.0: found negative value",
        ));
    }
    ctx.source().state.lock().unwrap().border.damage_per_block = amount;
    feedback(
        ctx,
        "commands.worldborder.damage.amount.success",
        vec![ferrite_command::error::java_float(amount)],
    )
}

fn worldborder_damage_buffer(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let distance = ctx
        .argument("distance")
        .and_then(ArgumentValue::as_f32)
        .ok_or_else(|| CommandError::failed("Missing damage buffer"))?;
    if distance < 0.0 {
        return Err(CommandError::failed(
            "Float must not be less than 0.0: found negative value",
        ));
    }
    ctx.source().state.lock().unwrap().border.damage_buffer = distance;
    feedback(
        ctx,
        "commands.worldborder.damage.buffer.success",
        vec![ferrite_command::error::java_float(distance)],
    )
}

fn worldborder_warning_distance(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let distance = ctx
        .integer("distance")
        .ok_or_else(|| CommandError::failed("Missing warning distance"))?;
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.border.warning_distance == distance {
            return Err(CommandError::failed(
                "Nothing changed. The world border warning is already that distance",
            ));
        }
        state.border.warning_distance = distance;
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BorderWarningDistance(SetBorderWarningDistance {
            warning_distance: ferrite_protocol::VarInt(distance),
        }),
    )?;
    feedback(
        ctx,
        "commands.worldborder.warning.distance.success",
        vec![distance.to_string()],
    )
}

fn worldborder_warning_time(ctx: &CommandContext<Source>) -> Result<i32, CommandError> {
    let ticks = ctx
        .integer("time")
        .ok_or_else(|| CommandError::failed("Missing warning time"))?;
    {
        let mut state = ctx.source().state.lock().unwrap();
        if state.border.warning_time == ticks {
            return Err(CommandError::failed(
                "Nothing changed. The world border warning is already that amount of time",
            ));
        }
        state.border.warning_time = ticks;
    }
    packets(
        ctx,
        Vec::new(),
        PlayerPacket::BorderWarningDelay(SetBorderWarningDelay {
            warning_delay: ferrite_protocol::VarInt(ticks),
        }),
    )?;
    feedback(
        ctx,
        "commands.worldborder.warning.time.success",
        vec![(ticks / 20).to_string()],
    )
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::commands::GameState;
    use super::*;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const ALICE: u128 = 1;

    fn source(permission: u8) -> (Source, UnboundedReceiver<Action>) {
        let mut state = GameState::default();
        state.join(Uuid::from_u128(ALICE), "Alice".into(), 1, 20);
        let (actions, received) = unbounded_channel();
        (
            Source {
                permission,
                origin: [0.0, 64.0, 0.0],
                yaw: 0.0,
                pitch: 0.0,
                uuid: Some(Uuid::from_u128(ALICE)),
                name: Some("Alice".into()),
                state: std::sync::Arc::new(std::sync::Mutex::new(state)),
                actions,
                anchor: ferrite_command::EntityAnchor::Feet,
                dimension: crate::game::Dimension::Overworld,
                world: None,
                tree: None,
                function_depth: 0,
            },
            received,
        )
    }

    fn run(input: &str, permission: u8) -> (Result<i32, CommandError>, Vec<Action>) {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(permission);
        let result = tree.execute(input, source);
        (
            result,
            std::iter::from_fn(|| received.try_recv().ok()).collect(),
        )
    }

    fn success(input: &str) -> Vec<Action> {
        let (result, actions) = run(input, 2);
        assert!(result.is_ok(), "{input}: {result:?}");
        actions
    }

    fn denied(input: &str) -> CommandError {
        let (result, actions) = run(input, 1);
        assert!(result.is_err(), "{input} should be denied");
        assert!(actions.is_empty(), "{input} produced {actions:?}");
        result.unwrap_err()
    }

    fn only_packet(actions: &[Action]) -> &PlayerPacket {
        actions
            .iter()
            .find_map(|action| match action {
                Action::Packets { packet, .. } => Some(packet),
                _ => None,
            })
            .expect("expected a packet action")
    }

    fn only_feedback(actions: &[Action]) -> &Feedback {
        actions
            .iter()
            .find_map(|action| match action {
                Action::Feedback(feedback) => Some(feedback),
                _ => None,
            })
            .expect("expected feedback")
    }

    fn error_text(error: CommandError) -> String {
        error.to_string()
    }

    #[test]
    fn effect_gives_and_clears() {
        let speed =
            ferrite_data::registries::id("minecraft:mob_effect", "minecraft:speed").unwrap();
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        tree.execute("effect give @s speed 5 1", source.clone())
            .unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Effect {
                effect,
                amplifier: 1,
                duration: 100,
                ..
            } if *effect == speed
        )));
        let feedback = only_feedback(&actions);
        assert_eq!(feedback.key, "commands.effect.give.success.single");
        assert_eq!(feedback.text, "Applied effect Speed to Alice");

        tree.execute("effect clear @s speed", source).unwrap();
        let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::RemoveEffect { effect, .. } if *effect == speed
        )));
        assert_eq!(
            only_feedback(&actions).key,
            "commands.effect.clear.specific.success.single"
        );

        let error = run("effect give @s not_an_effect 5", 2).0.unwrap_err();
        assert!(error_text(error).contains("not_an_effect"));
        assert!(run("effect clear @s speed", 2).0.is_err());
        denied("effect give @s speed 5");
    }

    #[test]
    fn enchant_applies_to_targets() {
        let actions = success("enchant @s sharpness 3");
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Enchant { enchantment, level: 3, .. }
                if enchantment == "minecraft:sharpness"
        )));
        let error = run("enchant @s not_an_enchantment 1", 2).0.unwrap_err();
        assert!(error_text(error).contains("not_an_enchantment"));
        denied("enchant @s sharpness 1");
    }

    #[test]
    fn title_subcommands_send_components_and_times() {
        let actions = success("title @s title {\"text\":\"Hello\"}");
        match only_packet(&actions) {
            PlayerPacket::Title(packet) => assert_eq!(packet.text.0, "Hello"),
            other => panic!("expected title, got {other:?}"),
        }
        assert_eq!(
            only_feedback(&actions).key,
            "commands.title.show.title.single"
        );

        let actions = success("title @s subtitle Sub");
        match only_packet(&actions) {
            PlayerPacket::Subtitle(packet) => assert_eq!(packet.text.0, "Sub"),
            other => panic!("expected subtitle, got {other:?}"),
        }
        let actions = success("title @s actionbar Bar");
        assert!(matches!(only_packet(&actions), PlayerPacket::ActionBar(_)));
        let actions = success("title @s times 10 70 20");
        assert!(matches!(
            only_packet(&actions),
            PlayerPacket::TitleTimes(SetTitlesAnimation {
                fade_in: 10,
                stay: 70,
                fade_out: 20
            })
        ));
        assert!(matches!(
            only_packet(&success("title @s clear")),
            PlayerPacket::ClearTitles(ClearTitles { reset: false })
        ));
        assert!(matches!(
            only_packet(&success("title @s reset")),
            PlayerPacket::ClearTitles(ClearTitles { reset: true })
        ));
        denied("title @s clear");
    }

    #[test]
    fn tellraw_sends_the_component_text() {
        let actions = success("tellraw @s {\"text\":\"Hello\",\"extra\":[\" world\"]}");
        match only_packet(&actions) {
            PlayerPacket::Tellraw(chat) => assert_eq!(chat.content.0, "Hello world"),
            other => panic!("expected tellraw, got {other:?}"),
        }
        denied("tellraw @s hello");
    }

    #[test]
    fn playsound_resolves_the_sound_and_category() {
        let actions = success("playsound minecraft:entity.player.levelup player @s ~ ~ ~ 1 1");
        match only_packet(&actions) {
            PlayerPacket::Sound(packet) => {
                assert_eq!(packet.source.0, 7);
                assert_eq!(packet.volume, 1.0);
            }
            other => panic!("expected sound, got {other:?}"),
        }
        assert_eq!(
            only_feedback(&actions).key,
            "commands.playsound.success.single"
        );
        let error = run("playsound not_a_sound master @s", 2).0.unwrap_err();
        assert!(error_text(error).contains("not_a_sound"));
        denied("playsound minecraft:entity.player.levelup master @s");
    }

    #[test]
    fn stopsound_sends_source_and_sound_filters() {
        let actions = success("stopsound @s music minecraft:music.game");
        match only_packet(&actions) {
            PlayerPacket::StopSound(packet) => {
                assert_eq!(packet.source.unwrap().0, 1);
                assert_eq!(packet.sound.as_deref(), Some("minecraft:music.game"));
            }
            other => panic!("expected stop sound, got {other:?}"),
        }
        assert_eq!(
            only_feedback(&actions).key,
            "commands.stopsound.success.source.sound"
        );
        let actions = success("stopsound @s *");
        match only_packet(&actions) {
            PlayerPacket::StopSound(packet) => {
                assert!(packet.source.is_none());
                assert!(packet.sound.is_none());
            }
            other => panic!("expected stop sound, got {other:?}"),
        }
        denied("stopsound @s");
    }

    #[test]
    fn particle_sends_position_delta_and_count() {
        let actions = success("particle minecraft:flame 1 64 1 0.1 0.2 0.3 0.5 12");
        match only_packet(&actions) {
            PlayerPacket::Particles(packet) => {
                assert_eq!(packet.x, 1.5);
                assert_eq!(packet.count, 12);
                assert_eq!(packet.speed, 0.5);
            }
            other => panic!("expected particles, got {other:?}"),
        }
        assert_eq!(only_feedback(&actions).key, "commands.particle.success");
        let error = run("particle not_a_particle", 2).0.unwrap_err();
        assert!(error_text(error).contains("not_a_particle"));
        denied("particle minecraft:flame");
    }

    #[test]
    fn spawnpoint_updates_player_state() {
        let (source, mut received) = source(2);
        let state = source.state.clone();
        let tree = super::super::commands::dispatcher();
        tree.execute("spawnpoint @s 10 70 20 90 10", source)
            .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.players[0].spawn, Some(([10, 70, 20], 90.0, 10.0)));
        drop(state);
        let feedback = loop {
            if let Ok(Action::Feedback(feedback)) = received.try_recv() {
                break feedback;
            }
        };
        assert_eq!(feedback.key, "commands.spawnpoint.success.single.new");
        assert!(
            feedback
                .text
                .starts_with("Set spawn point to 10, 70, 20 [90.0, 10.0] in minecraft:overworld")
        );
        denied("spawnpoint @s 10 70 20");
    }

    #[test]
    fn setworldspawn_updates_state_and_broadcasts() {
        let (source, mut received) = source(2);
        let state = source.state.clone();
        let tree = super::super::commands::dispatcher();
        tree.execute("setworldspawn 10 70 20 90 10", source)
            .unwrap();
        assert_eq!(state.lock().unwrap().world_spawn, [10, 70, 20]);
        let mut packet = None;
        let mut feedback = None;
        while let Ok(action) = received.try_recv() {
            match action {
                Action::Packets { packet: p, .. } => packet = Some(p),
                Action::Feedback(f) => feedback = Some(f),
                _ => {}
            }
        }
        assert!(matches!(packet, Some(PlayerPacket::DefaultSpawn(_))));
        assert_eq!(feedback.unwrap().key, "commands.setworldspawn.success.new");
        denied("setworldspawn 10 70 20");
    }

    #[test]
    fn worldborder_sets_centers_and_queries() {
        let tree = super::super::commands::dispatcher();
        let (source, mut received) = source(2);
        let mut execute = |input: &str| {
            tree.execute(input, source.clone()).unwrap();
            let actions: Vec<Action> = std::iter::from_fn(|| received.try_recv().ok()).collect();
            actions
        };
        let actions = execute("worldborder set 100");
        assert!(matches!(
            only_packet(&actions),
            PlayerPacket::BorderSize(SetBorderSize { diameter: 100.0 })
        ));
        assert_eq!(
            only_feedback(&actions).key,
            "commands.worldborder.set.immediate"
        );
        let actions = execute("worldborder add 20 40");
        assert!(matches!(only_packet(&actions), PlayerPacket::BorderLerp(_)));
        assert_eq!(only_feedback(&actions).key, "commands.worldborder.set.grow");
        let actions = execute("worldborder center 5 7");
        assert!(matches!(
            only_packet(&actions),
            PlayerPacket::BorderCenter(_)
        ));
        let actions = execute("worldborder get");
        assert!(only_feedback(&actions).text.contains("120.0"));
        let actions = execute("worldborder damage amount 0.5");
        assert_eq!(
            only_feedback(&actions).key,
            "commands.worldborder.damage.amount.success"
        );
        let actions = execute("worldborder damage buffer 3");
        assert_eq!(
            only_feedback(&actions).key,
            "commands.worldborder.damage.buffer.success"
        );
        let actions = execute("worldborder warning distance 8");
        assert!(matches!(
            only_packet(&actions),
            PlayerPacket::BorderWarningDistance(_)
        ));
        let actions = execute("worldborder warning time 40");
        assert!(matches!(
            only_packet(&actions),
            PlayerPacket::BorderWarningDelay(_)
        ));
        let (result, _) = run("worldborder set 0", 2);
        assert!(result.unwrap_err().to_string().contains("smaller than 1"));
        denied("worldborder get");
    }
}
