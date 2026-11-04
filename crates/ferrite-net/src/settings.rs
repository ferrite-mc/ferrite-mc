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
}
