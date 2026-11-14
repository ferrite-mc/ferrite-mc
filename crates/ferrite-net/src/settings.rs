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
    /// `ChatVisiblity` ordinal: 0 full, 1 system, 2 hidden.
    pub chat_mode: i32,
    pub chat_colors: bool,
    /// Bit set of the model parts shown (1 cape, 2 jacket, 4/8 sleeves, 16/32 pants, 64 hat).
    pub skin_parts: u8,
    pub text_filtering: bool,
    pub server_listing: bool,
    /// `ParticleStatus` ordinal: 0 all, 1 decreased, 2 minimal.
    pub particle_status: i32,
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
    virtual_hosts().lock().unwrap().remove(&player);
    pings().lock().unwrap().remove(&player);
    brands().lock().unwrap().remove(&player);
}

fn virtual_hosts() -> &'static Mutex<HashMap<Uuid, (String, u16)>> {
    static HOSTS: OnceLock<Mutex<HashMap<Uuid, (String, u16)>>> = OnceLock::new();
    HOSTS.get_or_init(Default::default)
}

/// The host name and port the client typed to connect (the handshake's `server_address` and
/// `server_port`), for `Player#getVirtualHost`. A BungeeCord handshake carries forwarding data
/// after the first NUL, which is not part of the name.
pub fn record_virtual_host(player: Uuid, address: &str, port: u16) {
    let host: String = address
        .split('\0')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect();
    if !host.is_empty() {
        virtual_hosts().lock().unwrap().insert(player, (host, port));
    }
}

pub fn virtual_host(player: Uuid) -> Option<(String, u16)> {
    virtual_hosts().lock().unwrap().get(&player).cloned()
}

fn brands() -> &'static Mutex<HashMap<Uuid, String>> {
    static BRANDS: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();
    BRANDS.get_or_init(Default::default)
}

/// Records the client brand from the `minecraft:brand` payload (a length-prefixed string).
pub fn record_brand_payload(player: Uuid, data: &[u8]) {
    let mut buf = data;
    let Ok(len) = <ferrite_protocol::VarInt as ferrite_protocol::Decode>::decode(&mut buf) else {
        return;
    };
    let Ok(len) = usize::try_from(len.0) else {
        return;
    };
    if len > 256 || buf.len() < len {
        return;
    }
    if let Ok(text) = std::str::from_utf8(&buf[..len]) {
        brands()
            .lock()
            .unwrap()
            .insert(player, text.chars().filter(|c| !c.is_control()).collect());
    }
}

/// The client brand (`vanilla`, `fabric`, ...), once reported.
pub fn brand(player: Uuid) -> Option<String> {
    brands().lock().unwrap().get(&player).cloned()
}

fn pings() -> &'static Mutex<HashMap<Uuid, i32>> {
    static PINGS: OnceLock<Mutex<HashMap<Uuid, i32>>> = OnceLock::new();
    PINGS.get_or_init(Default::default)
}

/// The smoothed round trip time of a player's connection in milliseconds, once measured.
pub fn ping(player: Uuid) -> Option<i32> {
    pings().lock().unwrap().get(&player).copied()
}

/// The smoothed keepalive round trip of every connected player that has one, for harnesses.
pub fn ping_snapshot() -> Vec<i32> {
    pings().lock().unwrap().values().copied().collect()
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
            chat_mode: 0,
            chat_colors: true,
            skin_parts: 0x7f,
            text_filtering: false,
            server_listing: true,
            particle_status: 0,
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
        record_brand_payload(player, &[6, b'f', b'a', b'b', b'r', b'i', b'c']);
        assert_eq!(brand(player).as_deref(), Some("fabric"));
        record_brand_payload(player, &[9, b'x']);
        assert_eq!(
            brand(player).as_deref(),
            Some("fabric"),
            "a truncated payload is ignored"
        );
        assert!(is_keepalive_answer(&[28, 0, 0, 0, 0, 0, 0, 0, 1]));
        assert!(!is_keepalive_answer(&[29]));
    }

    /// `Player#getVirtualHost`: what the client typed, without BungeeCord's forwarding data.
    #[test]
    fn the_virtual_host_is_the_handshake_name_and_port() {
        let player = Uuid::from_u128(0x7e57_4057);
        assert_eq!(virtual_host(player), None);
        record_virtual_host(player, "play.example.com", 25565);
        assert_eq!(
            virtual_host(player),
            Some(("play.example.com".to_owned(), 25565))
        );
        record_virtual_host(player, "lobby.example.com\x00203.0.113.7\0uuid\0[]", 25570);
        assert_eq!(
            virtual_host(player),
            Some(("lobby.example.com".to_owned(), 25570))
        );
        record_virtual_host(player, "\0only-forwarding", 1);
        assert_eq!(
            virtual_host(player).unwrap().1,
            25570,
            "an empty name keeps the old one"
        );
        forget(player);
        assert_eq!(virtual_host(player), None);
    }
}
