//! `/damage` damage sources: the damage type's data (`data/minecraft/damage_type/*.json`), who
//! caused the damage, and the death message vanilla builds from both.
//!
//! The rules here were read from the 26.3 server jar (`DamageSource`, `CombatTracker`,
//! `Player.hurtServer`) and checked against the generated damage type files:
//! - `by <entity>` is the direct *and* causing entity; `from <cause>` splits them
//!   (`DamageSource(type, direct, causing)`). Kill credit, aggro and the death message name the
//!   causing entity; knockback comes from the direct entity or the `at` position.
//! - A player victim scales the damage by difficulty when the type's `scaling` says so
//!   (`never`, `always`, or `when_caused_by_living_non_player`): peaceful 0, easy
//!   `min(a / 2 + 1, a)`, hard `a * 3 / 2`.
//! - The death message is `death.attack.<message_id>`, with the attacker's name as second
//!   argument when someone caused it, with `.item` when the causing living entity holds a
//!   custom-named item, and with `.player` plus the kill credit when nobody did.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ferrite_nbt::Tag;
use serde_json::Value;

/// `DamageScaling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    Never,
    WhenCausedByLivingNonPlayer,
    Always,
}

/// `DeathMessageType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathMessageType {
    Default,
    FallVariants,
    IntentionalGameDesign,
}

/// The data of one damage type file.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeProps {
    pub message_id: String,
    pub scaling: Scaling,
    pub exhaustion: f32,
    pub death_message_type: DeathMessageType,
}

impl TypeProps {
    /// Parses one `damage_type/<name>.json` file.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
        let message_id = value
            .get("message_id")
            .and_then(Value::as_str)
            .ok_or("damage type without message_id")?
            .to_owned();
        let scaling = match value.get("scaling").and_then(Value::as_str) {
            Some("never") => Scaling::Never,
            Some("always") => Scaling::Always,
            Some("when_caused_by_living_non_player") => Scaling::WhenCausedByLivingNonPlayer,
            other => return Err(format!("unknown damage scaling {other:?}")),
        };
        let death_message_type = match value.get("death_message_type").and_then(Value::as_str) {
            None | Some("default") => DeathMessageType::Default,
            Some("fall_variants") => DeathMessageType::FallVariants,
            Some("intentional_game_design") => DeathMessageType::IntentionalGameDesign,
            Some(other) => return Err(format!("unknown death_message_type {other}")),
        };
        Ok(Self {
            message_id,
            scaling,
            exhaustion: value
                .get("exhaustion")
                .and_then(Value::as_f64)
                .map_or(0.0, |v| v as f32),
            death_message_type,
        })
    }
}

/// Every damage type of the extracted vanilla data, keyed by bare name. Empty when the data is
/// not on this machine; [`props`] then answers `None` and callers fall back.
fn types() -> &'static BTreeMap<String, TypeProps> {
    static TYPES: OnceLock<BTreeMap<String, TypeProps>> = OnceLock::new();
    TYPES.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/data/minecraft/damage_type");
        let mut out = BTreeMap::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| TypeProps::parse(&text))
            {
                Ok(props) => {
                    out.insert(name.to_owned(), props);
                }
                Err(error) => tracing::warn!(%error, damage_type = name, "damage type skipped"),
            }
        }
        out
    })
}

/// The data of damage type `id` (`minecraft:` optional), or `None` for an unknown type or when
/// the vanilla data is missing.
pub fn props(id: &str) -> Option<TypeProps> {
    types()
        .get(id.strip_prefix("minecraft:").unwrap_or(id))
        .cloned()
}

/// Whether the damage type files are loaded at all.
pub fn data_available() -> bool {
    !types().is_empty()
}

/// An entity named in a damage source.
#[derive(Debug, Clone, PartialEq)]
pub struct Party {
    /// `getDisplayName()`: a player's name, a mob's custom name or type name.
    pub name: String,
    pub living: bool,
    pub player: bool,
    /// Display name of a custom-named main-hand item (`[Name]`), when the entity holds one.
    pub named_weapon: Option<String>,
}

/// `FallLocation`: where the victim was falling when the damage was recorded
/// (`FallLocation.getCurrentFallLocation`: the block of its last climbable position, or water).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallLocation {
    Generic,
    Ladder,
    Vines,
    WeepingVines,
    TwistingVines,
    Scaffolding,
    OtherClimbable,
    Water,
}

impl FallLocation {
    /// `FallLocation.id`, the end of `death.fell.accident.<id>`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Ladder => "ladder",
            Self::Vines => "vines",
            Self::WeepingVines => "weeping_vines",
            Self::TwistingVines => "twisting_vines",
            Self::Scaffolding => "scaffolding",
            Self::OtherClimbable => "other_climbable",
            Self::Water => "water",
        }
    }

    /// `FallLocation.blockToFallLocation` for the block a climbing victim last touched
    /// (`block` is a namespaced block id; `trapdoor` stands for the `trapdoors` block tag).
    pub fn of_climbable(block: &str) -> Self {
        let name = block.strip_prefix("minecraft:").unwrap_or(block);
        match name {
            "ladder" => Self::Ladder,
            "vine" => Self::Vines,
            "weeping_vines" | "weeping_vines_plant" => Self::WeepingVines,
            "twisting_vines" | "twisting_vines_plant" => Self::TwistingVines,
            "scaffolding" => Self::Scaffolding,
            _ if name.ends_with("_trapdoor") => Self::Ladder,
            _ => Self::OtherClimbable,
        }
    }
}

/// The `CombatEntry` data the fall death messages read: `CombatTracker.recordDamage` stores the
/// victim's fall distance, the fall location and the damage with the source.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FallInfo {
    /// `LivingEntity.fallDistance` when the damage was recorded.
    pub distance: f32,
    /// The damage dealt.
    pub damage: f32,
    /// `None` when the victim was neither climbing nor in water.
    pub location: Option<FallLocation>,
}

impl FallInfo {
    /// `CombatTracker.getMostSignificantFall` for a victim whose only combat entry is a
    /// `#minecraft:is_fall` hit (`fall`, `ender_pearl`, `stalagmite`): the fall entry wins with
    /// a distance over 5; otherwise the entry counts when it has a fall location and more than
    /// 5 damage.
    pub fn is_significant(&self) -> bool {
        self.distance > 5.0 || (self.location.is_some() && self.damage > 5.0)
    }
}

/// `DamageSource`: the type plus the direct and causing entities.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub type_id: String,
    pub direct: Option<Party>,
    pub causing: Option<Party>,
    /// The victim's fall data. The default (nothing fallen, not climbing) is right for
    /// `/damage` on a standing victim; callers that know better set it with [`Self::with_fall`].
    pub fall: FallInfo,
}

impl Source {
    /// `/damage ... by <entity>` has `direct == causing`; `from <cause>` sets them apart;
    /// without either the damage has no entity.
    pub fn new(type_id: &str, direct: Option<Party>, causing: Option<Party>) -> Self {
        let causing = causing.or_else(|| direct.clone());
        Self {
            type_id: type_id.to_owned(),
            direct,
            causing,
            fall: FallInfo::default(),
        }
    }

    /// The same source with the victim's recorded fall data.
    pub fn with_fall(mut self, fall: FallInfo) -> Self {
        self.fall = fall;
        self
    }

    /// `DamageSource.scalesWithDifficulty`.
    pub fn scales_with_difficulty(&self, props: &TypeProps) -> bool {
        match props.scaling {
            Scaling::Never => false,
            Scaling::Always => true,
            Scaling::WhenCausedByLivingNonPlayer => self
                .causing
                .as_ref()
                .is_some_and(|party| party.living && !party.player),
        }
    }
}

/// `Player.hurtServer`'s difficulty scaling (`difficulty` is 0 peaceful .. 3 hard).
pub fn scale_for_difficulty(amount: f32, difficulty: i32) -> f32 {
    match difficulty {
        0 => 0.0,
        1 => (amount / 2.0 + 1.0).min(amount),
        3 => amount * 3.0 / 2.0,
        _ => amount,
    }
}

/// The translation key and arguments of the death message
/// (`DamageSource.getLocalizedDeathMessage`; `CombatTracker.getDeathMessage` for the
/// `intentional_game_design` type). `kill_credit` is the name of whoever last hurt the victim,
/// used when the source names no entity.
///
/// `fall_variants` (`fall`, `ender_pearl`) follow `CombatTracker.getDeathMessage` for a victim
/// whose only combat entry is this hit: a significant fall (see [`FallInfo::is_significant`])
/// reads `death.fell.accident.<location>`, anything else the plain `death.attack.<message_id>`.
/// Ferrite keeps no combat history, so the `death.fell.assist` / `finish` / `killer` messages,
/// which need an earlier attacker, are not produced.
pub fn death_message_key(
    props: &TypeProps,
    victim: &str,
    source: &Source,
    kill_credit: Option<&str>,
) -> (String, Vec<String>) {
    let base = format!("death.attack.{}", props.message_id);
    if props.death_message_type == DeathMessageType::FallVariants && source.fall.is_significant() {
        let location = source.fall.location.unwrap_or(FallLocation::Generic);
        return (
            format!("death.fell.accident.{}", location.id()),
            vec![victim.to_owned()],
        );
    }
    if props.death_message_type == DeathMessageType::IntentionalGameDesign {
        // The message links to the Minecraft Wiki page of the same name; only the text is kept.
        let link = format!("[{}]", link_text(&props.message_id));
        return (format!("{base}.message"), vec![victim.to_owned(), link]);
    }
    if source.causing.is_some() || source.direct.is_some() {
        let who = source.causing.as_ref().or(source.direct.as_ref());
        let name = who.map(|party| party.name.clone()).unwrap_or_default();
        let weapon = source
            .causing
            .as_ref()
            .filter(|party| party.living)
            .and_then(|party| party.named_weapon.clone());
        return match weapon {
            Some(item) => (format!("{base}.item"), vec![victim.to_owned(), name, item]),
            None => (base, vec![victim.to_owned(), name]),
        };
    }
    match kill_credit {
        Some(killer) => (
            format!("{base}.player"),
            vec![victim.to_owned(), killer.to_owned()],
        ),
        None => (base, vec![victim.to_owned()]),
    }
}

/// The link label of an `intentional_game_design` message (`death.attack.<id>.link`).
fn link_text(message_id: &str) -> String {
    super::commands::translate_key(&format!("death.attack.{message_id}.link"), &[])
        .unwrap_or_else(|| "Intentional Game Design".to_owned())
}

/// The rendered death message in English (vanilla's `en_us`), or the key and its arguments when
/// the translation table is not loaded.
pub fn death_message(
    props: &TypeProps,
    victim: &str,
    source: &Source,
    kill_credit: Option<&str>,
) -> String {
    let (key, args) = death_message_key(props, victim, source, kill_credit);
    super::commands::translate_key(&key, &args)
        .unwrap_or_else(|| format!("{key}: {}", args.join(", ")))
}

/// The plain text of a custom name component (a string, or the `text` of a compound).
pub fn plain_text(tag: &Tag) -> Option<String> {
    match tag {
        Tag::String(text) => {
            // A JSON-encoded component: `"Name"` or `{"text":"Name"}`.
            match serde_json::from_str::<Value>(text) {
                Ok(Value::String(plain)) => Some(plain),
                Ok(Value::Object(object)) => object
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => Some(text.clone()),
            }
        }
        Tag::Compound(compound) => match compound.get("text") {
            Some(Tag::String(text)) => Some(text.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> bool {
        data_available()
    }

    fn party(name: &str, living: bool, player: bool) -> Party {
        Party {
            name: name.to_owned(),
            living,
            player,
            named_weapon: None,
        }
    }

    fn generic() -> TypeProps {
        TypeProps::parse(
            r#"{"exhaustion":0.0,"message_id":"generic","scaling":"when_caused_by_living_non_player"}"#,
        )
        .unwrap()
    }

    #[test]
    fn type_files_parse_and_reject_unknown_values() {
        let props = TypeProps::parse(
            r#"{"death_message_type":"fall_variants","exhaustion":0.1,"message_id":"fall","scaling":"always"}"#,
        )
        .unwrap();
        assert_eq!(props.message_id, "fall");
        assert_eq!(props.scaling, Scaling::Always);
        assert_eq!(props.death_message_type, DeathMessageType::FallVariants);
        assert_eq!(props.exhaustion, 0.1);
        assert!(TypeProps::parse(r#"{"message_id":"x","scaling":"sometimes"}"#).is_err());
        assert!(TypeProps::parse(r#"{"scaling":"never"}"#).is_err());
    }

    #[test]
    fn by_is_direct_and_causing_and_from_splits_them() {
        let zombie = party("Zombie", true, false);
        let alice = party("Alice", true, true);
        let by = Source::new("minecraft:mob_attack", Some(zombie.clone()), None);
        assert_eq!(by.causing, Some(zombie.clone()));
        let from = Source::new("minecraft:arrow", Some(zombie.clone()), Some(alice.clone()));
        assert_eq!(from.direct, Some(zombie));
        assert_eq!(from.causing, Some(alice));
        let none = Source::new("minecraft:generic", None, None);
        assert!(none.direct.is_none() && none.causing.is_none());
    }

    #[test]
    fn scaling_follows_the_type_and_the_causing_entity() {
        let props = generic();
        let by_mob = Source::new("g", Some(party("Zombie", true, false)), None);
        let by_player = Source::new("g", Some(party("Alice", true, true)), None);
        let by_item = Source::new("g", Some(party("Arrow", false, false)), None);
        assert!(by_mob.scales_with_difficulty(&props));
        assert!(!by_player.scales_with_difficulty(&props));
        assert!(!by_item.scales_with_difficulty(&props));
        assert!(!Source::new("g", None, None).scales_with_difficulty(&props));
        let always = TypeProps {
            scaling: Scaling::Always,
            ..props.clone()
        };
        assert!(Source::new("g", None, None).scales_with_difficulty(&always));
        let never = TypeProps {
            scaling: Scaling::Never,
            ..props
        };
        assert!(!by_mob.scales_with_difficulty(&never));
    }

    #[test]
    fn difficulty_scales_like_player_hurt_server() {
        assert_eq!(scale_for_difficulty(6.0, 0), 0.0);
        assert_eq!(scale_for_difficulty(6.0, 1), 4.0);
        assert_eq!(
            scale_for_difficulty(1.0, 1),
            1.0,
            "easy never exceeds the amount"
        );
        assert_eq!(scale_for_difficulty(6.0, 2), 6.0);
        assert_eq!(scale_for_difficulty(6.0, 3), 9.0);
    }

    #[test]
    fn death_message_keys_follow_the_attacker_and_weapon() {
        let props = TypeProps::parse(
            r#"{"exhaustion":0.1,"message_id":"mob","scaling":"when_caused_by_living_non_player"}"#,
        )
        .unwrap();
        let zombie = Source::new(
            "minecraft:mob_attack",
            Some(party("Zombie", true, false)),
            None,
        );
        assert_eq!(
            death_message_key(&props, "Alice", &zombie, None),
            (
                "death.attack.mob".to_owned(),
                vec!["Alice".into(), "Zombie".into()]
            )
        );
        let mut armed = party("Bob", true, true);
        armed.named_weapon = Some("[Excalibur]".into());
        let source = Source::new("minecraft:mob_attack", Some(armed), None);
        assert_eq!(
            death_message_key(&props, "Alice", &source, None),
            (
                "death.attack.mob.item".to_owned(),
                vec!["Alice".into(), "Bob".into(), "[Excalibur]".into()]
            )
        );
        // A named weapon only counts for a living causing entity.
        let mut arrow = party("Arrow", false, false);
        arrow.named_weapon = Some("[x]".into());
        let source = Source::new("minecraft:mob_attack", Some(arrow), None);
        assert_eq!(
            death_message_key(&props, "Alice", &source, None).0,
            "death.attack.mob"
        );
        // No entity: the kill credit adds `.player`, otherwise the bare key.
        let none = Source::new("minecraft:mob_attack", None, None);
        assert_eq!(
            death_message_key(&props, "Alice", &none, Some("Bob")),
            (
                "death.attack.mob.player".to_owned(),
                vec!["Alice".into(), "Bob".into()]
            )
        );
        assert_eq!(
            death_message_key(&props, "Alice", &none, None),
            ("death.attack.mob".to_owned(), vec!["Alice".into()])
        );
    }

    #[test]
    fn intentional_game_design_links_instead_of_naming_an_attacker() {
        let props = TypeProps::parse(
            r#"{"death_message_type":"intentional_game_design","exhaustion":0.1,"message_id":"badRespawnPoint","scaling":"always"}"#,
        )
        .unwrap();
        let source = Source::new(
            "minecraft:bad_respawn_point",
            Some(party("Zombie", true, false)),
            None,
        );
        let (key, args) = death_message_key(&props, "Alice", &source, None);
        assert_eq!(key, "death.attack.badRespawnPoint.message");
        assert_eq!(args[0], "Alice");
        assert!(args[1].starts_with('[') && args[1].ends_with(']'));
    }

    #[test]
    fn fall_variants_read_the_recorded_fall() {
        // CombatTracker.getMostSignificantFall / getFallMessage, read from the 26.3 jar.
        let props = TypeProps {
            message_id: "fall".into(),
            scaling: Scaling::Always,
            exhaustion: 0.0,
            death_message_type: DeathMessageType::FallVariants,
        };
        let key = |fall: FallInfo| {
            death_message_key(
                &props,
                "Alice",
                &Source::new("minecraft:fall", None, None).with_fall(fall),
                None,
            )
            .0
        };
        // Nothing fallen: the plain message, as for `/damage` on a standing victim.
        assert_eq!(key(FallInfo::default()), "death.attack.fall");
        // A fall of exactly 5 is not significant; over 5 it is.
        let fall = |distance, damage, location| FallInfo {
            distance,
            damage,
            location,
        };
        assert_eq!(key(fall(5.0, 2.0, None)), "death.attack.fall");
        assert_eq!(key(fall(5.5, 3.0, None)), "death.fell.accident.generic");
        // A location makes a hit of more than 5 damage count even after a short fall.
        assert_eq!(
            key(fall(1.0, 6.0, Some(FallLocation::Ladder))),
            "death.fell.accident.ladder"
        );
        assert_eq!(
            key(fall(1.0, 5.0, Some(FallLocation::Ladder))),
            "death.attack.fall"
        );
        assert_eq!(
            key(fall(9.0, 6.0, Some(FallLocation::Water))),
            "death.fell.accident.water"
        );
        assert_eq!(
            FallLocation::of_climbable("minecraft:ladder"),
            FallLocation::Ladder
        );
        assert_eq!(
            FallLocation::of_climbable("minecraft:oak_trapdoor"),
            FallLocation::Ladder
        );
        assert_eq!(
            FallLocation::of_climbable("minecraft:twisting_vines_plant"),
            FallLocation::TwistingVines
        );
        assert_eq!(
            FallLocation::of_climbable("minecraft:cave_vines"),
            FallLocation::OtherClimbable
        );
    }

    #[test]
    fn every_fall_location_message_exists_in_the_language_file() {
        // The 26.3 language file has no `death.fell.accident.water` (vanilla emits the key
        // anyway, so a client shows it raw); the other locations all have a translation.
        if !lang() {
            return;
        }
        for location in [
            FallLocation::Generic,
            FallLocation::Ladder,
            FallLocation::Vines,
            FallLocation::WeepingVines,
            FallLocation::TwistingVines,
            FallLocation::Scaffolding,
            FallLocation::OtherClimbable,
        ] {
            let key = format!("death.fell.accident.{}", location.id());
            assert!(
                super::super::commands::translate_key(&key, &["x".into()]).is_some(),
                "{key} missing from the language file"
            );
        }
    }

    #[test]
    fn every_vanilla_damage_type_has_its_message_id_and_properties() {
        if !data() {
            return;
        }
        let all = ferrite_data::registries::data_entries("damage_type").expect("registry");
        assert_eq!(all.len(), 51);
        for id in all {
            let props = props(id).unwrap_or_else(|| panic!("{id} did not load"));
            assert!(!props.message_id.is_empty(), "{id}");
            assert!(props.exhaustion == 0.0 || props.exhaustion == 0.1, "{id}");
        }
        let mob = props("minecraft:mob_attack").unwrap();
        assert_eq!(mob.message_id, "mob");
        let player = props("player_attack").unwrap();
        assert_eq!(player.message_id, "player");
        assert_eq!(
            props("minecraft:fall").unwrap().death_message_type,
            DeathMessageType::FallVariants
        );
        assert_eq!(props("bad_respawn_point").unwrap().scaling, Scaling::Always);
        assert!(props("minecraft:not_a_type").is_none());
    }

    /// Loads vanilla's English table once for the tests that render messages.
    fn lang() -> bool {
        let reports = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vanilla/26.3/generated/reports");
        super::super::commands::load_lang(&reports);
        super::super::commands::translate_key("death.attack.generic", &["x".into()]).is_some()
    }

    #[test]
    fn death_messages_render_the_vanilla_english_text() {
        if !data() || !lang() {
            return;
        }
        let mob = props("minecraft:mob_attack").unwrap();
        let zombie = Source::new(
            "minecraft:mob_attack",
            Some(party("Zombie", true, false)),
            None,
        );
        assert_eq!(
            death_message(&mob, "Alice", &zombie, None),
            "Alice was slain by Zombie"
        );
        let mut armed = party("Bob", true, true);
        armed.named_weapon = Some("[Excalibur]".into());
        let player = props("minecraft:player_attack").unwrap();
        let source = Source::new("minecraft:player_attack", Some(armed), None);
        assert_eq!(
            death_message(&player, "Alice", &source, None),
            "Alice was slain by Bob using [Excalibur]"
        );
        let generic = props("generic").unwrap();
        let none = Source::new("minecraft:generic", None, None);
        assert_eq!(death_message(&generic, "Alice", &none, None), "Alice died");
        assert_eq!(
            death_message(&generic, "Alice", &none, Some("Bob")),
            "Alice died because of Bob"
        );
        // `from`: the shooter, not the arrow, is named.
        let arrow = props("arrow").unwrap();
        let shot = Source::new(
            "minecraft:arrow",
            Some(party("Arrow", false, false)),
            Some(party("Skeleton", true, false)),
        );
        assert_eq!(
            death_message(&arrow, "Alice", &shot, None),
            "Alice was shot by Skeleton"
        );
        let design = props("bad_respawn_point").unwrap();
        let text = death_message(&design, "Alice", &none, None);
        assert!(
            text.starts_with("Alice was killed by [Intentional Game Design]"),
            "{text}"
        );
    }

    #[test]
    fn every_damage_type_message_exists_in_the_language_file() {
        if !data() || !lang() {
            return;
        }
        for id in ferrite_data::registries::data_entries("damage_type").unwrap() {
            let props = props(id).unwrap();
            let key = match props.death_message_type {
                DeathMessageType::IntentionalGameDesign => {
                    format!("death.attack.{}.message", props.message_id)
                }
                _ => format!("death.attack.{}", props.message_id),
            };
            assert!(
                super::super::commands::translate_key(&key, &[]).is_some(),
                "{id}: {key} is not in lang_en_us.json"
            );
        }
    }

    #[test]
    fn damage_kind_agrees_with_the_bypasses_invulnerability_tag() {
        // Cross-check of the 13 hand-written `DamageKind`s against the 51 data-driven types: the
        // creative-mode exemption must match the vanilla tag exactly.
        for id in ferrite_data::registries::data_entries("damage_type").unwrap() {
            let tagged =
                ferrite_data::tags::contains("damage_type", "bypasses_invulnerability", id);
            let kind = super::super::damage_kind(id);
            assert_eq!(
                kind.bypasses_invulnerability(),
                tagged,
                "{id}: {kind:?} vs tag"
            );
        }
    }

    #[test]
    fn custom_names_read_as_plain_text() {
        assert_eq!(
            plain_text(&Tag::String("Plain".into())).as_deref(),
            Some("Plain")
        );
        assert_eq!(
            plain_text(&Tag::String(r#"{"text":"Json"}"#.into())).as_deref(),
            Some("Json")
        );
        assert_eq!(
            plain_text(&Tag::String(r#""Quoted""#.into())).as_deref(),
            Some("Quoted")
        );
        assert_eq!(plain_text(&Tag::Int(3)), None);
    }
}
