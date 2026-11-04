//! What the client reported in its `ClientInformation` packet during configuration, kept per
//! player so plugin queries (locale, client view distance, main hand) can be answered.
//!
//! Settings the client changes later in play are not tracked: only the configuration values.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientSettings {
    /// The language tag the client sent, such as `en_us`.
    pub locale: String,
    /// The client's render distance in chunks.
    pub view_distance: i8,
    /// Whether the client's main hand is the left one.
    pub left_handed: bool,
}

fn registry() -> &'static Mutex<HashMap<Uuid, ClientSettings>> {
    static REGISTRY: OnceLock<Mutex<HashMap<Uuid, ClientSettings>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

pub fn record(player: Uuid, settings: ClientSettings) {
    registry().lock().unwrap().insert(player, settings);
}

pub fn get(player: Uuid) -> Option<ClientSettings> {
    registry().lock().unwrap().get(&player).cloned()
}

pub fn forget(player: Uuid) {
    registry().lock().unwrap().remove(&player);
    pings().lock().unwrap().remove(&player);
}

fn pings() -> &'static Mutex<HashMap<Uuid, i32>> {
    static PINGS: OnceLock<Mutex<HashMap<Uuid, i32>>> = OnceLock::new();
    PINGS.get_or_init(Default::default)
}

/// The smoothed round trip time of a player's connection in milliseconds, once measured.
pub fn ping(player: Uuid) -> Option<i32> {
    pings().lock().unwrap().get(&player).copied()
}

/// Measures the round trip of the play keepalive: the writer marks `sent`, the reader marks
/// `answered`, and the result is smoothed like vanilla (`(old * 3 + new) / 4`).
#[derive(Debug)]
pub struct PingProbe {
    player: Uuid,
    sent: Mutex<Option<std::time::Instant>>,
}

impl PingProbe {
    pub fn new(player: Uuid) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            player,
            sent: Mutex::new(None),
        })
    }

    /// A keepalive left; an earlier unanswered one keeps its older timestamp.
    pub fn sent(&self) {
        self.sent
            .lock()
            .unwrap()
            .get_or_insert_with(std::time::Instant::now);
    }

    /// The client answered the keepalive.
    pub fn answered(&self) {
        let Some(at) = self.sent.lock().unwrap().take() else {
            return;
        };
        let rtt = i32::try_from(at.elapsed().as_millis()).unwrap_or(i32::MAX);
        let mut pings = pings().lock().unwrap();
        let next = match pings.get(&self.player) {
            Some(old) => ((i64::from(*old) * 3 + i64::from(rtt)) / 4) as i32,
            None => rtt,
        };
        pings.insert(self.player, next);
    }
}

/// Whether a serverbound play frame is the keepalive answer.
pub fn is_keepalive_answer(frame: &[u8]) -> bool {
    let mut buf = frame;
    matches!(<ferrite_protocol::VarInt as ferrite_protocol::Decode>::decode(&mut buf), Ok(id) if id.0 == 28)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_kept_until_forgotten() {
        let player = Uuid::from_u128(0x5e77);
        assert_eq!(get(player), None);
        let settings = ClientSettings {
            locale: "ko_kr".into(),
            view_distance: 12,
            left_handed: true,
        };
        record(player, settings.clone());
        assert_eq!(get(player), Some(settings));
        forget(player);
        assert_eq!(get(player), None);
    }

    #[test]
    fn ping_is_measured_from_the_keepalive_round_trip_and_smoothed() {
        let player = Uuid::from_u128(0x9179);
        assert_eq!(ping(player), None);
        let probe = PingProbe::new(player);
        probe.answered();
        assert_eq!(
            ping(player),
            None,
            "an answer without a keepalive is ignored"
        );
        probe.sent();
        std::thread::sleep(std::time::Duration::from_millis(20));
        probe.answered();
        let first = ping(player).unwrap();
        assert!((20..2000).contains(&first), "{first}");
        forget(player);
        assert_eq!(ping(player), None);
        assert!(is_keepalive_answer(&[28, 0, 0, 0, 0, 0, 0, 0, 1]));
        assert!(!is_keepalive_answer(&[29]));
    }
}
