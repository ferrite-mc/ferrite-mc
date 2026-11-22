//! Village raids: a player with Bad Omen near a villager starts a raid, waves of raiders spawn
//! around the village, and clearing the last wave makes every surviving raider celebrate
//! (`Raider.IS_CELEBRATING`, which includes `Witch.CELEBRATING`) and grants nearby players Hero
//! of the Village.
//!
//! The deterministic wave rules live in [`ferrite_entity::raid`]. A raid is owned by the region
//! that holds its village; `split`/`merge` move it with that region. Raider deaths are ordinary
//! mob deaths, so the existing AI, loot and metadata paths apply.

use std::collections::HashMap;

use ferrite_entity::effect::{Effect, MobEffectInstance};
use ferrite_entity::mob::{self, MobKind};
use ferrite_entity::{BlockGetter, EntityKey, EntityStore, Flags, Rng};
use ferrite_math::Vec3;
use uuid::Uuid;

use super::Dimension;
use super::region::Player;

use ferrite_entity::raid as rules;
pub use ferrite_entity::raid::{
    BAD_OMEN_DURATION, CELEBRATION_TICKS, HERO_DURATION, RAID_TIMEOUT_TICKS, SPAWN_RADIUS,
    VILLAGE_RANGE, WAVE_BREAK_TICKS,
};

/// Where a raid is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidStatus {
    Ongoing,
    /// The last wave died: raiders celebrate and nearby players get Hero of the Village.
    Victory,
    /// The raid timed out; its remaining raiders are left to the ordinary mob rules.
    Lost,
    /// The celebration finished and the raid has been removed.
    Finished,
}

/// One live raid.
#[derive(Debug, Clone)]
pub struct Raid {
    pub center: [i32; 3],
    pub dimension: Dimension,
    /// Zero-based wave the raid is fighting.
    pub wave: u32,
    /// Waves this raid was started with (`Bad Omen` level + 3).
    pub total_waves: u32,
    pub status: RaidStatus,
    /// Ticks since the current wave was spawned or since the victory started.
    pub ticks: u32,
    /// Ticks left before the next wave may spawn; `None` while a wave is still alive.
    pub next_wave: Option<u32>,
    /// Every raider this raid spawned, across all waves.
    pub spawned: Vec<Uuid>,
}

impl Raid {
    fn new(center: [i32; 3], dimension: Dimension, amplifier: i32) -> Self {
        Self {
            center,
            dimension,
            wave: 0,
            total_waves: rules::wave_count(amplifier),
            status: RaidStatus::Ongoing,
            ticks: 0,
            next_wave: None,
            spawned: Vec::new(),
        }
    }
}

/// All raids a region holds.
#[derive(Debug, Default)]
pub struct RaidManager {
    raids: Vec<Raid>,
}

impl RaidManager {
    /// True when the point lies inside an existing raid's village.
    pub fn near(&self, pos: [i32; 3], dimension: Dimension) -> bool {
        self.raids.iter().any(|raid| {
            raid.dimension == dimension && horizontal_distance(raid.center, pos) < VILLAGE_RANGE
        })
    }

    /// One region tick: starts raids for Bad Omen players and advances or finishes the live
    /// raids of this dimension.
    pub fn tick(
        &mut self,
        store: &mut EntityStore,
        players: &mut HashMap<Uuid, Player>,
        rng: &mut Rng,
        blocks: &impl BlockGetter,
        dimension: Dimension,
    ) {
        // The common case (no raid and nobody carrying Bad Omen) skips the village scan
        // entirely; this runs every region tick.
        if self.raids.is_empty() && !players.values().any(|p| p.effects.has(Effect::BadOmen)) {
            return;
        }
        self.start_ready(store, players, rng, blocks, dimension);
        for raid in &mut self.raids {
            if raid.dimension != dimension {
                continue;
            }
            match raid.status {
                RaidStatus::Ongoing => {
                    raid.ticks = raid.ticks.saturating_add(1);
                    if raid.ticks >= RAID_TIMEOUT_TICKS {
                        raid.status = RaidStatus::Lost;
                        continue;
                    }
                    if raiders_alive(store, &raid.spawned) {
                        continue;
                    }
                    match raid.next_wave {
                        Some(wait) if wait > 1 => raid.next_wave = Some(wait - 1),
                        Some(_) => {
                            raid.wave += 1;
                            raid.next_wave = None;
                            spawn_wave(raid, store, rng, blocks);
                        }
                        None if raid.wave + 1 >= raid.total_waves => {
                            begin_victory(raid, store, players);
                        }
                        None => raid.next_wave = Some(WAVE_BREAK_TICKS),
                    }
                }
                RaidStatus::Victory => {
                    raid.ticks = raid.ticks.saturating_add(1);
                    if raid.ticks >= CELEBRATION_TICKS {
                        stop_celebration(raid, store);
                        raid.status = RaidStatus::Finished;
                    }
                }
                RaidStatus::Lost | RaidStatus::Finished => {}
            }
        }
        self.raids
            .retain(|raid| !matches!(raid.status, RaidStatus::Finished | RaidStatus::Lost));
    }

    fn start_ready(
        &mut self,
        store: &mut EntityStore,
        players: &mut HashMap<Uuid, Player>,
        rng: &mut Rng,
        blocks: &impl BlockGetter,
        dimension: Dimension,
    ) {
        if dimension != Dimension::Overworld {
            return;
        }
        let mut starts = Vec::new();
        for player in players.values_mut() {
            if !player.effects.has(Effect::BadOmen) {
                continue;
            }
            let amplifier = player
                .effects
                .get(Effect::BadOmen)
                .map_or(0, |instance| instance.amplifier);
            let Some(village) = nearest_village(store, player.pos) else {
                continue;
            };
            let center = [
                village.x.floor() as i32,
                village.y.floor() as i32,
                village.z.floor() as i32,
            ];
            if self.near(center, dimension) {
                continue;
            }
            // Vanilla consumes the effect when the raid starts (`Raids.startRaid`).
            player.remove_effect(Effect::BadOmen);
            starts.push((center, amplifier));
        }
        for (center, amplifier) in starts {
            let mut raid = Raid::new(center, dimension, amplifier);
            spawn_wave(&mut raid, store, rng, blocks);
            self.raids.push(raid);
        }
    }

    /// Moves the raids whose village belongs to a split-off region.
    pub fn split(&mut self, take: impl Fn([i32; 3]) -> bool) -> Self {
        let (moving, staying) = std::mem::take(&mut self.raids)
            .into_iter()
            .partition(|raid: &Raid| take(raid.center));
        self.raids = staying;
        Self { raids: moving }
    }

    /// Absorbs the raids of a merged region.
    pub fn merge(&mut self, mut other: Self) {
        self.raids.append(&mut other.raids);
    }
}

fn horizontal_distance(a: [i32; 3], b: [i32; 3]) -> f64 {
    let dx = f64::from(a[0] - b[0]);
    let dz = f64::from(a[2] - b[2]);
    dx.hypot(dz)
}

fn nearest_village(store: &EntityStore, at: [f64; 3]) -> Option<Vec3> {
    let mut best: Option<(f64, Vec3)> = None;
    for &key in store.keys() {
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if state.kind != MobKind::Villager || state.is_dead() {
            continue;
        }
        let Some(pos) = store.pos(key) else {
            continue;
        };
        if (pos.y - at[1]).abs() > 48.0 {
            continue;
        }
        let distance = (pos.x - at[0]).hypot(pos.z - at[2]);
        if distance > VILLAGE_RANGE {
            continue;
        }
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, pos));
        }
    }
    best.map(|(_, pos)| pos)
}

fn spawn_wave(raid: &mut Raid, store: &mut EntityStore, rng: &mut Rng, blocks: &impl BlockGetter) {
    let raiders = rules::wave_raiders(raid.wave);
    let count = raiders.len().max(1) as f64;
    for (index, kind) in raiders.into_iter().enumerate() {
        let angle = std::f64::consts::TAU * index as f64 / count;
        let x = f64::from(raid.center[0]) + 0.5 + angle.cos() * SPAWN_RADIUS;
        let z = f64::from(raid.center[2]) + 0.5 + angle.sin() * SPAWN_RADIUS;
        let y = surface_y(blocks, x.floor() as i32, z.floor() as i32, raid.center[1]);
        let mut record = mob::new_mob(kind, Vec3::new(x, y, z), rng);
        if let Some(state) = record.data.mob.as_mut() {
            // The first raider of the first wave carries the raid's Bad Omen.
            state.raid_captain = raid.wave == 0 && index == 0;
        }
        let weapon = match kind {
            MobKind::Pillager => Some("minecraft:crossbow"),
            MobKind::Vindicator => Some("minecraft:iron_axe"),
            _ => None,
        };
        if let Some(weapon) = weapon
            && let Some(stack) = ferrite_item::ItemStack::from_name(weapon)
        {
            record
                .data
                .equipment
                .set(ferrite_entity::EquipmentSlot::MainHand, Some(stack));
        }
        mob::sync_state_metadata(&mut record.data);
        raid.spawned.push(record.data.uuid);
        store.insert(record);
    }
}

/// The highest block surface near `around` (within 24 above and 32 below).
fn surface_y(blocks: &impl BlockGetter, x: i32, z: i32, around: i32) -> f64 {
    for y in (around - 32..=around + 24).rev() {
        let Some((name, _)) = blocks
            .block_state(x, y, z)
            .and_then(ferrite_data::blocks::state)
        else {
            continue;
        };
        let air = name == "minecraft:air"
            || name == "minecraft:cave_air"
            || name == "minecraft:void_air"
            || name.ends_with("_air");
        let fluid = name.ends_with("water") || name.ends_with("lava");
        if !air && !fluid {
            return f64::from(y + 1);
        }
    }
    f64::from(around)
}

fn raiders_alive(store: &EntityStore, spawned: &[Uuid]) -> bool {
    store.keys().iter().any(|&key| {
        let Some(data) = store.data(key) else {
            return false;
        };
        if !spawned.contains(&data.uuid)
            || store
                .flags(key)
                .is_some_and(|flags| flags.has(Flags::REMOVED))
        {
            return false;
        }
        data.mob.is_some_and(|state| !state.is_dead())
    })
}

fn entity_key(store: &EntityStore, uuid: Uuid) -> Option<EntityKey> {
    store
        .keys()
        .iter()
        .copied()
        .find(|&key| store.data(key).is_some_and(|data| data.uuid == uuid))
}

fn begin_victory(raid: &mut Raid, store: &mut EntityStore, players: &mut HashMap<Uuid, Player>) {
    raid.status = RaidStatus::Victory;
    raid.ticks = 0;
    for &uuid in &raid.spawned {
        let Some(key) = entity_key(store, uuid) else {
            continue;
        };
        if !mob::state(store, key).is_some_and(|state| !state.is_dead()) {
            continue;
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.celebrating = true;
        }
        if let Some(data) = store.data_mut(key) {
            mob::sync_state_metadata(data);
        }
    }
    let (center_x, center_z) = (f64::from(raid.center[0]), f64::from(raid.center[2]));
    for player in players.values_mut() {
        let dx = player.pos[0] - center_x;
        let dz = player.pos[2] - center_z;
        if player.gamemode != 3 && dx.hypot(dz) <= VILLAGE_RANGE {
            player.apply_effect(MobEffectInstance::new(
                Effect::HeroOfTheVillage.id(),
                HERO_DURATION,
                0,
            ));
        }
    }
}

fn stop_celebration(raid: &Raid, store: &mut EntityStore) {
    for &uuid in &raid.spawned {
        let Some(key) = entity_key(store, uuid) else {
            continue;
        };
        if !mob::state(store, key).is_some_and(|state| state.celebrating) {
            continue;
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.celebrating = false;
        }
        if let Some(data) = store.data_mut(key) {
            mob::sync_state_metadata(data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ground;

    impl BlockGetter for Ground {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            (y <= 63).then(|| ferrite_data::blocks::default_state("stone").unwrap())
        }
    }

    #[test]
    fn waves_spawn_on_the_block_surface() {
        assert_eq!(surface_y(&Ground, 0, 0, 64), 64.0);
    }

    #[test]
    fn an_empty_manager_does_no_work() {
        let mut manager = RaidManager::default();
        let mut store = EntityStore::default();
        let mut players = HashMap::new();
        let mut rng = Rng::new(1);
        manager.tick(
            &mut store,
            &mut players,
            &mut rng,
            &Ground,
            Dimension::Overworld,
        );
        assert!(manager.raids.is_empty());
    }

    #[test]
    fn split_and_merge_keep_a_raid_with_its_village() {
        let mut manager = RaidManager::default();
        manager
            .raids
            .push(Raid::new([0, 64, 0], Dimension::Overworld, 0));
        let other = manager.split(|pos| pos[0] >= 0);
        assert!(manager.raids.is_empty());
        assert_eq!(other.raids.len(), 1);
        manager.merge(other);
        assert_eq!(manager.raids.len(), 1);
        assert!(manager.near([10, 64, 10], Dimension::Overworld));
        assert!(!manager.near([10, 64, 10], Dimension::Nether));
    }
}
