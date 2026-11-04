//! Facts about an online player that only the game loop knows, answered to plugin queries:
//! the network entity id, the protocol version, when the session began and how long it has run.
//!
//! The game loop records a player when they join and forgets them when they leave; plugin
//! threads only read. A query about a player who is not recorded gets no answer, so the plugin
//! sees the usual "not implemented" error instead of an invented value.

use ferrite_plugin::HostCall;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerFacts {
    pub entity_id: i32,
    pub joined: SystemTime,
    /// The connecting address (the forwarded one behind a proxy); the port is not kept.
    pub address: Option<std::net::IpAddr>,
}

fn registry() -> &'static Mutex<HashMap<Uuid, PlayerFacts>> {
    static REGISTRY: OnceLock<Mutex<HashMap<Uuid, PlayerFacts>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

fn ground_registry() -> &'static Mutex<HashMap<Uuid, bool>> {
    static GROUND: OnceLock<Mutex<HashMap<Uuid, bool>>> = OnceLock::new();
    GROUND.get_or_init(Default::default)
}

fn sleeping_registry() -> &'static Mutex<std::collections::HashSet<Uuid>> {
    static SLEEPING: OnceLock<Mutex<std::collections::HashSet<Uuid>>> = OnceLock::new();
    SLEEPING.get_or_init(Default::default)
}

/// Hunger, air and fire values the survival tick last computed for a player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub saturation: f32,
    pub exhaustion: f32,
    pub absorption: f32,
    pub air: i32,
    pub fire_ticks: i32,
}

fn vitals_registry() -> &'static Mutex<HashMap<Uuid, Vitals>> {
    static VITALS: OnceLock<Mutex<HashMap<Uuid, Vitals>>> = OnceLock::new();
    VITALS.get_or_init(Default::default)
}

pub fn set_vitals(player: Uuid, vitals: Vitals) {
    vitals_registry().lock().unwrap().insert(player, vitals);
}

fn vitals_answer(vitals: &Vitals, method: &str) -> Option<String> {
    Some(match method {
        "getSaturation()" => vitals.saturation.to_string(),
        "getExhaustion()" => vitals.exhaustion.to_string(),
        "getAbsorptionAmount()" => f64::from(vitals.absorption).to_string(),
        "getRemainingAir()" => vitals.air.to_string(),
        "getFireTicks()" => vitals.fire_ticks.max(0).to_string(),
        _ => return None,
    })
}

/// What the client last said about its keys and sprinting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Controls {
    /// `Input` flags: 1 forward, 2 backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint.
    input: u8,
    sprinting: bool,
}

fn controls_registry() -> &'static Mutex<HashMap<Uuid, Controls>> {
    static CONTROLS: OnceLock<Mutex<HashMap<Uuid, Controls>>> = OnceLock::new();
    CONTROLS.get_or_init(Default::default)
}

/// Records the movement keys from a `player_input` packet.
pub fn set_input(player: Uuid, flags: u8) {
    controls_registry()
        .lock()
        .unwrap()
        .entry(player)
        .or_default()
        .input = flags;
}

/// Records a start or stop of sprinting from a `player_command` packet.
pub fn set_sprinting(player: Uuid, sprinting: bool) {
    controls_registry()
        .lock()
        .unwrap()
        .entry(player)
        .or_default()
        .sprinting = sprinting;
}

fn controls_answer(controls: &Controls, method: &str) -> Option<String> {
    Some(match method {
        "isSneaking()" => (controls.input & 32 != 0).to_string(),
        "isSprinting()" => (controls.sprinting || controls.input & 64 != 0).to_string(),
        "getInputFlags()" => controls.input.to_string(),
        _ => return None,
    })
}

fn fall_registry() -> &'static Mutex<HashMap<Uuid, f32>> {
    static FALL: OnceLock<Mutex<HashMap<Uuid, f32>>> = OnceLock::new();
    FALL.get_or_init(Default::default)
}

/// Records how far the player has fallen since last standing on something.
pub fn set_fall_distance(player: Uuid, distance: f32) {
    fall_registry().lock().unwrap().insert(player, distance);
}

fn history_registry() -> &'static Mutex<HashMap<Uuid, (bool, SystemTime)>> {
    static HISTORY: OnceLock<Mutex<HashMap<Uuid, (bool, SystemTime)>>> = OnceLock::new();
    HISTORY.get_or_init(Default::default)
}

/// Records whether the player had joined before this session and when they first did.
pub fn set_history(player: Uuid, played_before: bool, first_played: SystemTime) {
    history_registry()
        .lock()
        .unwrap()
        .insert(player, (played_before, first_played));
}

/// Records that the player went to bed or got up.
pub fn set_sleeping(player: Uuid, sleeping: bool) {
    let mut set = sleeping_registry().lock().unwrap();
    if sleeping {
        set.insert(player);
    } else {
        set.remove(&player);
    }
}

/// Records whether the client last reported the player standing on the ground.
pub fn set_on_ground(player: Uuid, on_ground: bool) {
    ground_registry().lock().unwrap().insert(player, on_ground);
}

pub fn record(player: Uuid, facts: PlayerFacts) {
    registry().lock().unwrap().insert(player, facts);
}

/// The network entity id of an online player.
pub fn entity_id(player: Uuid) -> Option<i32> {
    registry().lock().unwrap().get(&player).map(|f| f.entity_id)
}

pub fn forget(player: Uuid) {
    registry().lock().unwrap().remove(&player);
    ground_registry().lock().unwrap().remove(&player);
    sleeping_registry().lock().unwrap().remove(&player);
    fall_registry().lock().unwrap().remove(&player);
    controls_registry().lock().unwrap().remove(&player);
    vitals_registry().lock().unwrap().remove(&player);
    history_registry().lock().unwrap().remove(&player);
    ferrite_net::settings::forget(player);
}

fn millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

/// The answer to a read-only query, given the facts and the current time.
fn answer_at(facts: &PlayerFacts, method: &str, now: SystemTime) -> Option<String> {
    Some(match method {
        "getEntityId()" => facts.entity_id.to_string(),
        "getProtocolVersion()" => ferrite_protocol::PROTOCOL_VERSION.to_string(),
        "getTicksLived()" => {
            let elapsed = now.duration_since(facts.joined).unwrap_or(Duration::ZERO);
            (elapsed.as_millis() / 50).min(i32::MAX as u128).to_string()
        }
        "getLastLogin()" => millis(facts.joined).to_string(),
        // An online player is being seen and is playing right now.
        "getLastSeen()" | "getLastPlayed()" => millis(now).to_string(),
        "isInWorld()" | "isTicking()" => "true".to_owned(),
        "getAddressText()" => facts.address?.to_string(),
        _ => return None,
    })
}

/// The answer to a query about what the client reported in configuration.
fn client_setting(
    settings: &ferrite_net::settings::ClientSettings,
    method: &str,
) -> Option<String> {
    Some(match method {
        "getLocale()" | "locale()" => settings.locale.clone(),
        "getClientViewDistance()" => settings.view_distance.max(0).to_string(),
        "getMainHand()" => if settings.left_handed {
            "LEFT"
        } else {
            "RIGHT"
        }
        .to_owned(),
        _ => return None,
    })
}

/// The answer to `call`, or `None` when it is not one of these queries or the player is unknown.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Player" || !call.args.is_empty() {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    if call.method == "isOnGround()" {
        return ground_registry()
            .lock()
            .unwrap()
            .get(&player)
            .map(|ground| ground.to_string());
    }
    if call.method == "isSleeping()" {
        // Only an online player has a bed state to report.
        registry().lock().unwrap().get(&player)?;
        return Some(
            sleeping_registry()
                .lock()
                .unwrap()
                .contains(&player)
                .to_string(),
        );
    }
    if matches!(
        call.method.as_str(),
        "hasPlayedBefore()" | "getFirstPlayed()"
    ) {
        let (played_before, first) = *history_registry().lock().unwrap().get(&player)?;
        return Some(if call.method == "hasPlayedBefore()" {
            played_before.to_string()
        } else {
            millis(first).to_string()
        });
    }
    if let Some(vitals) = vitals_registry().lock().unwrap().get(&player)
        && let Some(reply) = vitals_answer(vitals, &call.method)
    {
        return Some(reply);
    }
    if matches!(
        call.method.as_str(),
        "isSneaking()" | "isSprinting()" | "getInputFlags()"
    ) {
        // An online player who has sent no keys yet is standing still.
        registry().lock().unwrap().get(&player)?;
        let controls = controls_registry()
            .lock()
            .unwrap()
            .get(&player)
            .copied()
            .unwrap_or_default();
        return controls_answer(&controls, &call.method);
    }
    if call.method == "getFallDistance()" {
        return fall_registry()
            .lock()
            .unwrap()
            .get(&player)
            .map(|distance| distance.to_string());
    }
    if call.method == "getClientBrandName()" {
        return ferrite_net::settings::brand(player);
    }
    if call.method == "getPing()"
        && let Some(ping) = ferrite_net::settings::ping(player)
    {
        return Some(ping.to_string());
    }
    if let Some(settings) = ferrite_net::settings::get(player)
        && let Some(reply) = client_setting(&settings, &call.method)
    {
        return Some(reply);
    }
    let facts = *registry().lock().unwrap().get(&player)?;
    answer_at(&facts, &call.method, SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(player: Uuid, method: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: player.to_string(),
            method: method.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn sleeping_state_follows_the_game_loop_for_online_players() {
        let player = Uuid::from_u128(0x51ee);
        let call = HostCall {
            owner: "Player".into(),
            subject: player.to_string(),
            method: "isSleeping()".into(),
            args: vec![],
        };
        assert_eq!(answer(&call), None);
        record(
            player,
            PlayerFacts {
                entity_id: 1,
                joined: SystemTime::now(),
                address: None,
            },
        );
        assert_eq!(answer(&call).as_deref(), Some("false"));
        set_sleeping(player, true);
        assert_eq!(answer(&call).as_deref(), Some("true"));
        set_sleeping(player, false);
        assert_eq!(answer(&call).as_deref(), Some("false"));
        forget(player);
        assert_eq!(answer(&call), None);
    }

    #[test]
    fn fall_distance_is_answered_until_the_player_is_forgotten() {
        let player = Uuid::from_u128(0xfa11);
        let call = HostCall {
            owner: "Player".into(),
            subject: player.to_string(),
            method: "getFallDistance()".into(),
            args: vec![],
        };
        assert_eq!(answer(&call), None);
        set_fall_distance(player, 3.5);
        assert_eq!(answer(&call).as_deref(), Some("3.5"));
        forget(player);
        assert_eq!(answer(&call), None);
    }

    #[test]
    fn join_history_is_answered_from_what_the_game_loop_recorded() {
        let player = Uuid::from_u128(0x41570);
        let ask = |method: &str| {
            answer(&HostCall {
                owner: "Player".into(),
                subject: player.to_string(),
                method: method.into(),
                args: vec![],
            })
        };
        assert_eq!(ask("hasPlayedBefore()"), None);
        set_history(player, true, UNIX_EPOCH + Duration::from_millis(1234));
        assert_eq!(ask("hasPlayedBefore()").as_deref(), Some("true"));
        assert_eq!(ask("getFirstPlayed()").as_deref(), Some("1234"));
        forget(player);
        assert_eq!(ask("getFirstPlayed()"), None);
    }

    #[test]
    fn vitals_are_answered_from_the_last_survival_tick() {
        let player = Uuid::from_u128(0x7a15);
        let ask = |method: &str| {
            answer(&HostCall {
                owner: "Player".into(),
                subject: player.to_string(),
                method: method.into(),
                args: vec![],
            })
        };
        assert_eq!(ask("getSaturation()"), None);
        set_vitals(
            player,
            Vitals {
                saturation: 5.0,
                exhaustion: 1.5,
                absorption: 4.0,
                air: 120,
                fire_ticks: -1,
            },
        );
        assert_eq!(ask("getSaturation()").as_deref(), Some("5"));
        assert_eq!(ask("getExhaustion()").as_deref(), Some("1.5"));
        assert_eq!(ask("getAbsorptionAmount()").as_deref(), Some("4"));
        assert_eq!(ask("getRemainingAir()").as_deref(), Some("120"));
        assert_eq!(ask("getFireTicks()").as_deref(), Some("0"));
        forget(player);
        assert_eq!(ask("getRemainingAir()"), None);
    }

    #[test]
    fn keys_and_sprinting_are_answered_for_online_players() {
        let player = Uuid::from_u128(0x1e75);
        let ask = |method: &str| {
            answer(&HostCall {
                owner: "Player".into(),
                subject: player.to_string(),
                method: method.into(),
                args: vec![],
            })
        };
        assert_eq!(ask("isSneaking()"), None);
        record(
            player,
            PlayerFacts {
                entity_id: 1,
                joined: SystemTime::now(),
                address: None,
            },
        );
        assert_eq!(ask("isSneaking()").as_deref(), Some("false"));
        assert_eq!(ask("getInputFlags()").as_deref(), Some("0"));
        set_input(player, 1 | 32);
        assert_eq!(ask("isSneaking()").as_deref(), Some("true"));
        assert_eq!(ask("getInputFlags()").as_deref(), Some("33"));
        assert_eq!(ask("isSprinting()").as_deref(), Some("false"));
        set_sprinting(player, true);
        assert_eq!(ask("isSprinting()").as_deref(), Some("true"));
        set_sprinting(player, false);
        set_input(player, 64);
        assert_eq!(ask("isSprinting()").as_deref(), Some("true"));
        forget(player);
        assert_eq!(ask("isSneaking()"), None);
    }

    #[test]
    fn ground_state_is_answered_after_the_client_reports_it() {
        let player = Uuid::from_u128(0x6a0d);
        let call = HostCall {
            owner: "Player".into(),
            subject: player.to_string(),
            method: "isOnGround()".into(),
            args: vec![],
        };
        assert_eq!(answer(&call), None);
        set_on_ground(player, false);
        assert_eq!(answer(&call).as_deref(), Some("false"));
        forget(player);
        assert_eq!(answer(&call), None);
    }

    #[test]
    fn recorded_players_answer_and_forgotten_ones_do_not() {
        let player = Uuid::from_u128(0xfac7);
        assert_eq!(answer(&call(player, "getEntityId()")), None);
        record(
            player,
            PlayerFacts {
                entity_id: 77,
                joined: SystemTime::now(),
                address: None,
            },
        );
        assert_eq!(
            answer(&call(player, "getEntityId()")).as_deref(),
            Some("77")
        );
        assert_eq!(
            answer(&call(player, "getProtocolVersion()")).as_deref(),
            Some(ferrite_protocol::PROTOCOL_VERSION.to_string().as_str())
        );
        forget(player);
        assert_eq!(answer(&call(player, "getEntityId()")), None);
    }

    #[test]
    fn client_settings_answer_locale_view_distance_and_hand() {
        let settings = ferrite_net::settings::ClientSettings {
            locale: "ko_kr".into(),
            view_distance: 12,
            left_handed: true,
        };
        assert_eq!(
            client_setting(&settings, "getLocale()").as_deref(),
            Some("ko_kr")
        );
        assert_eq!(
            client_setting(&settings, "getClientViewDistance()").as_deref(),
            Some("12")
        );
        assert_eq!(
            client_setting(&settings, "getMainHand()").as_deref(),
            Some("LEFT")
        );
        assert_eq!(client_setting(&settings, "getPing()"), None);
    }

    #[test]
    fn ticks_lived_counts_fifty_milliseconds_each() {
        let joined = UNIX_EPOCH + Duration::from_secs(1_000);
        let facts = PlayerFacts {
            entity_id: 1,
            joined,
            address: None,
        };
        let later = joined + Duration::from_millis(2_025);
        assert_eq!(
            answer_at(&facts, "getTicksLived()", later).as_deref(),
            Some("40")
        );
        assert_eq!(
            answer_at(&facts, "getLastLogin()", later).as_deref(),
            Some("1000000")
        );
        assert_eq!(
            answer_at(&facts, "getLastSeen()", later).as_deref(),
            Some("1002025")
        );
        assert_eq!(answer_at(&facts, "getPing()", later), None);
    }

    #[test]
    fn queries_with_arguments_are_not_facts() {
        let player = Uuid::from_u128(0xfac8);
        record(
            player,
            PlayerFacts {
                entity_id: 5,
                joined: SystemTime::now(),
                address: None,
            },
        );
        let mut with_arg = call(player, "getEntityId()");
        with_arg.args.push("x".into());
        assert_eq!(answer(&with_arg), None);
        forget(player);
    }

    #[test]
    fn the_address_is_answered_as_text_when_known() {
        let now = UNIX_EPOCH;
        let mut facts = PlayerFacts {
            entity_id: 1,
            joined: now,
            address: Some("203.0.113.9".parse().unwrap()),
        };
        assert_eq!(
            answer_at(&facts, "getAddressText()", now).as_deref(),
            Some("203.0.113.9")
        );
        facts.address = None;
        assert_eq!(answer_at(&facts, "getAddressText()", now), None);
    }
}
