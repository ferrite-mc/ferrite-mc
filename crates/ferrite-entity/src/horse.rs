//! Horses (`AbstractHorse`): an untamed horse can be ridden, bucks its rider off now and then
//! and is tamed with a chance that grows with each ride (its temper); a tamed horse takes a
//! saddle and is then steered by its rider. Horses, donkeys and mules share this. shortcut: no
//! llamas, armour, chest or inventory, jump strength or speed genetics, or eating of treats.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `AbstractHorse.DATA_ID_FLAGS` (read from the 26.3 server classes).
pub const FLAGS_INDEX: u8 = 17;
/// Synched `Horse.DATA_ID_TYPE_VARIANT`.
pub const VARIANT_INDEX: u8 = 18;
const TAME: i8 = 2;
/// `AbstractHorse.getMaxTemper`.
const MAX_TEMPER: i32 = 100;
/// Temper gained by every ride that did not tame (`RunAroundLikeCrazyGoal`).
const TEMPER_STEP: i32 = 5;
/// The `EquipmentSlot` index of the saddle in a `set_equipment` packet.
pub const SADDLE_SLOT: u8 = 7;
/// The most health an equine is born with (`AbstractHorse.generateMaxHealth`).
const MAX_HEALTH: f32 = 30.0;
/// The `EquipmentSlot` index of body armour (horse armour).
pub const BODY_SLOT: u8 = 6;

/// `(health, temper)` a treat gives (`AbstractHorse.handleEating`).
fn treat_values(item: &str) -> Option<(f32, i32)> {
    Some(match item {
        "minecraft:sugar" => (1.0, 3),
        "minecraft:wheat" => (2.0, 3),
        "minecraft:apple" | "minecraft:golden_carrot" => (
            if item == "minecraft:apple" { 3.0 } else { 4.0 },
            if item == "minecraft:apple" { 3 } else { 5 },
        ),
        "minecraft:golden_apple" | "minecraft:enchanted_golden_apple" => (10.0, 10),
        "minecraft:hay_block" => (20.0, 3),
        _ => return None,
    })
}

/// Feeds a treat: heals the horse and makes a wild one trust riders more. `true` when it did
/// something, so the item is used up.
pub fn treat(store: &mut EntityStore, key: EntityKey, item: &str) -> bool {
    let Some((heal, temper)) = treat_values(item) else {
        return false;
    };
    let Some(state) = mob::state_mut(store, key) else {
        return false;
    };
    if !state.kind.is_equine() {
        return false;
    }
    let max = state.kind.max_health().min(MAX_HEALTH);
    let wild = state.owner.is_none() && state.temper < MAX_TEMPER;
    if state.health >= max && !wild {
        return false;
    }
    state.health = (state.health + heal).min(max);
    if state.owner.is_none() {
        state.temper = (state.temper + temper).min(MAX_TEMPER);
    }
    true
}

/// Whether an item is horse armour.
pub fn is_armor(item: &str) -> bool {
    item.ends_with("_horse_armor")
}

/// Armour points of a horse armour item (`Items.*_HORSE_ARMOR` armor attribute).
pub fn armor_points(item: &str) -> f32 {
    match item {
        "minecraft:leather_horse_armor" => 3.0,
        "minecraft:copper_horse_armor" => 4.0,
        "minecraft:iron_horse_armor" => 5.0,
        "minecraft:golden_horse_armor" => 7.0,
        "minecraft:diamond_horse_armor" => 11.0,
        "minecraft:netherite_horse_armor" => 19.0,
        _ => 0.0,
    }
}

/// Puts armour (an item id) on a tame horse that wears none. `false` when it does not apply.
pub fn wear_armor(store: &mut EntityStore, key: EntityKey, item: u16, points: f32) -> bool {
    match mob::state_mut(store, key) {
        Some(s) if s.kind == MobKind::Horse && s.owner.is_some() && s.armor == 0 => {
            s.armor = item;
            s.armor_points = points;
            true
        }
        _ => false,
    }
}

/// Defines the horse data values and writes the current state into them.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(FLAGS_INDEX, MetaValue::Byte(0));
    // Index 18 is a horse's coat; donkeys and mules carry a chest flag there instead.
    if state.kind == MobKind::Horse {
        metadata.define(VARIANT_INDEX, MetaValue::Int(0));
    } else {
        metadata.define(VARIANT_INDEX, MetaValue::Boolean(false));
    }
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let flags = if state.owner.is_some() { TAME } else { 0 };
    metadata.set(FLAGS_INDEX, MetaValue::Byte(flags));
}

fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
    {
        write_metadata(&mut data.metadata, &state);
    }
}

/// Whether the horse is tame and saddled, so its rider steers it.
pub fn is_steerable(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key).is_some_and(|s| s.kind.is_equine() && s.owner.is_some() && s.saddled)
}

/// Puts a saddle on a tame, unsaddled horse. `false` when it does not apply.
pub fn saddle(store: &mut EntityStore, key: EntityKey) -> bool {
    match mob::state_mut(store, key) {
        Some(s) if s.kind.is_equine() && s.owner.is_some() && !s.saddled => {
            s.saddled = true;
            true
        }
        _ => false,
    }
}

/// What an untamed horse with a rider did this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucked {
    /// The rider stays on.
    No,
    /// The horse threw the rider off and stayed wild.
    Wild,
    /// The horse accepted the rider and is tame now.
    Tamed,
}

/// `RunAroundLikeCrazyGoal`, shortened: now and then an untamed horse with a rider rolls its
/// temper against the maximum. It is tamed on success, otherwise the temper grows; either way
/// the rider falls off. shortcut: a plain 1-in-50 chance per tick instead of the goal's timing.
pub fn tick_rider(store: &mut EntityStore, key: EntityKey, rider: Uuid, rng: &mut Rng) -> Bucked {
    let Some(state) = mob::state_mut(store, key) else {
        return Bucked::No;
    };
    if !state.kind.is_equine() || state.owner.is_some() || rng.next_int(50) != 0 {
        return Bucked::No;
    }
    if rng.next_int(MAX_TEMPER) < state.temper {
        state.owner = Some(rider);
        sync(store, key);
        Bucked::Tamed
    } else {
        state.temper += TEMPER_STEP;
        Bucked::Wild
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::new_mob;
    use ferrite_math::Vec3;

    fn horse(store: &mut EntityStore) -> EntityKey {
        store.insert(new_mob(
            MobKind::Horse,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ))
    }

    #[test]
    fn a_wild_horse_is_tamed_after_enough_rides_and_then_takes_a_saddle() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        let rider = Uuid::from_u128(5);
        assert!(!saddle(&mut store, key), "a wild horse takes no saddle");
        let mut rng = Rng::new(7);
        let mut tamed = false;
        for _ in 0..200_000 {
            if tick_rider(&mut store, key, rider, &mut rng) == Bucked::Tamed {
                tamed = true;
                break;
            }
        }
        assert!(tamed);
        assert_eq!(mob::state(&store, key).unwrap().owner, Some(rider));
        assert_eq!(
            store.data(key).unwrap().metadata.get(FLAGS_INDEX),
            Some(&MetaValue::Byte(TAME))
        );
        assert!(!is_steerable(&store, key));
        assert!(saddle(&mut store, key));
        assert!(!saddle(&mut store, key));
        assert!(is_steerable(&store, key));
        assert!(wear_armor(
            &mut store,
            key,
            7,
            armor_points("minecraft:iron_horse_armor")
        ));
        assert!(
            !wear_armor(&mut store, key, 8, 3.0),
            "one set of armour at a time"
        );
        let before = mob::state(&store, key).unwrap().health;
        mob::hurt(&mut store, key, 10.0, None);
        let lost = before - mob::state(&store, key).unwrap().health;
        assert!(
            lost < 10.0 && lost > 7.0,
            "iron armour absorbs part of a hit, lost {lost}"
        );
    }

    #[test]
    fn treats_heal_and_raise_the_temper_of_a_wild_horse_only_while_useful() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        assert!(!treat(&mut store, key, "minecraft:stone"));
        assert!(treat(&mut store, key, "minecraft:golden_apple"));
        assert_eq!(mob::state(&store, key).unwrap().temper, 10);
        mob::state_mut(&mut store, key).unwrap().owner = Some(Uuid::from_u128(1));
        assert!(
            !treat(&mut store, key, "minecraft:wheat"),
            "a tame horse at full health"
        );
        mob::state_mut(&mut store, key).unwrap().health = 5.0;
        assert!(treat(&mut store, key, "minecraft:wheat"));
        assert_eq!(mob::state(&store, key).unwrap().health, 7.0);
    }
}
