//! Real `minecraft:lightning_bolt` entity lifecycle and vanilla entity conversions
//! (`Creeper` -> charged, `Mooshroom` color toggle, `Pig` -> `ZombifiedPiglin`, `Villager` -> `Witch`).

use crate::mob::{self, MobKind};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use crate::{allocate_id, random_uuid};
use ferrite_math::{AABB, Vec3};
use std::sync::OnceLock;

/// Ticks a lightning bolt stays alive before despawning (`LightningBolt.life`).
pub const LIGHTNING_LIFETIME: u32 = 6;
/// Vanilla lightning bolt strike radius (`3.0` blocks horizontally/vertically).
pub const STRIKE_RADIUS: f64 = 3.0;
/// Vanilla lightning bolt direct damage (`5.0`).
pub const STRIKE_DAMAGE: f32 = 5.0;

/// Entity type id of `minecraft:lightning_bolt`.
pub fn lightning_type() -> u16 {
    static KIND: OnceLock<u16> = OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("lightning_bolt")
            .expect("vanilla has a lightning_bolt entity type")
            .id
    })
}

/// Constructs a new `minecraft:lightning_bolt` entity at `pos`.
pub fn new_lightning(pos: Vec3) -> EntityRecord {
    let data = EntityData::new(allocate_id(), random_uuid(), lightning_type(), 0.0, 0.0);
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
    }
}

/// Spawns a `minecraft:lightning_bolt` entity at `pos` and applies vanilla `Entity.thunderHit`
/// conversions and 5.0 damage + fire to nearby mobs. Returns names of struck mob types.
pub fn strike(store: &mut EntityStore, pos: Vec3, rng: &mut Rng) -> Vec<&'static str> {
    store.insert(new_lightning(pos));
    let area = AABB::new(
        pos.x - STRIKE_RADIUS,
        pos.y - STRIKE_RADIUS,
        pos.z - STRIKE_RADIUS,
        pos.x + STRIKE_RADIUS,
        pos.y + STRIKE_RADIUS,
        pos.z + STRIKE_RADIUS,
    );
    let near = store.query_vec(area);
    let mut victims = Vec::new();
    let mut replacements: Vec<(MobKind, Vec3)> = Vec::new();
    for key in near {
        if !mob::is_mob(store, key) {
            continue;
        }
        let Some(kind) = mob::state(store, key).map(|s| s.kind) else {
            continue;
        };
        victims.push(kind.name());
        match kind {
            MobKind::Creeper => {
                crate::creeper::thunder_hit(store, key);
                mob::hurt(store, key, STRIKE_DAMAGE, None);
            }
            MobKind::Mooshroom => {
                if let Some(state) = mob::state_mut(store, key) {
                    state.variant = Some(1 - state.variant.unwrap_or(0));
                }
                if let Some(data) = store.data_mut(key) {
                    mob::sync_state_metadata(data);
                }
                mob::hurt(store, key, STRIKE_DAMAGE, None);
            }
            MobKind::Pig => {
                if let Some(at) = store.pos(key) {
                    if let Some(flags) = store.flags_mut(key) {
                        flags.set(Flags::REMOVED, true);
                    }
                    replacements.push((MobKind::ZombifiedPiglin, at));
                }
            }
            MobKind::Villager => {
                if let Some(at) = store.pos(key) {
                    if let Some(flags) = store.flags_mut(key) {
                        flags.set(Flags::REMOVED, true);
                    }
                    replacements.push((MobKind::Witch, at));
                }
            }
            _ => {
                mob::hurt(store, key, STRIKE_DAMAGE, None);
                if !kind.is_fire_immune() {
                    mob::ignite(store, key, 160);
                }
            }
        }
    }
    for (new_kind, at) in replacements {
        store.insert(mob::new_mob(new_kind, at, rng));
    }
    victims
}

/// Advances all active `minecraft:lightning_bolt` entities and removes them after [`LIGHTNING_LIFETIME`] ticks.
pub fn tick_lightning(store: &mut EntityStore) {
    if store.is_empty() {
        return;
    }
    let bolt_kind = lightning_type();
    let keys = store.keys().to_vec();
    for key in keys {
        let Some(data) = store.data_mut(key) else {
            continue;
        };
        if data.kind != bolt_kind {
            continue;
        }
        data.tick_count = data.tick_count.saturating_add(1);
        if data.tick_count >= LIGHTNING_LIFETIME
            && let Some(flags) = store.flags_mut(key)
        {
            flags.set(Flags::REMOVED, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightning_bolt_converts_pig_villager_creeper_and_expires() {
        let mut store = EntityStore::default();
        let mut rng = Rng::new(7);
        let at = Vec3::new(0.5, 64.0, 0.5);
        let pig = store.insert(mob::new_mob(MobKind::Pig, at, &mut rng));
        let villager = store.insert(mob::new_mob(
            MobKind::Villager,
            Vec3::new(1.5, 64.0, 0.5),
            &mut rng,
        ));
        let creeper = store.insert(mob::new_mob(
            MobKind::Creeper,
            Vec3::new(-1.0, 64.0, 0.5),
            &mut rng,
        ));

        let victims = strike(&mut store, at, &mut rng);
        assert_eq!(victims.len(), 3);
        assert!(store.flags(pig).unwrap().has(Flags::REMOVED));
        assert!(store.flags(villager).unwrap().has(Flags::REMOVED));
        assert!(mob::state(&store, creeper).unwrap().powered);

        let has_zpig = store
            .keys()
            .iter()
            .any(|&k| mob::state(&store, k).is_some_and(|s| s.kind == MobKind::ZombifiedPiglin));
        let has_witch = store
            .keys()
            .iter()
            .any(|&k| mob::state(&store, k).is_some_and(|s| s.kind == MobKind::Witch));
        assert!(has_zpig, "struck pig becomes zombified piglin");
        assert!(has_witch, "struck villager becomes witch");

        let bolt = store
            .keys()
            .iter()
            .copied()
            .find(|&k| store.data(k).is_some_and(|d| d.kind == lightning_type()))
            .expect("lightning_bolt entity spawned");
        for _ in 0..LIGHTNING_LIFETIME {
            tick_lightning(&mut store);
        }
        assert!(store.flags(bolt).unwrap().has(Flags::REMOVED));
    }
}
