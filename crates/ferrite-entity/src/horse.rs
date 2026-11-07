//! Equines (`AbstractHorse`): wild animals can be tamed by riding; tame equines support saddles,
//! horse armour, pack chests, llama carpets and treats. Horse movement speed, jump strength and
//! health are randomized at spawn. Bucking follows `RunAroundLikeCrazyGoal`: while a wild equine
//! carries a rider it rolls on the mob goal phase, every other game tick.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `AbstractHorse.DATA_ID_FLAGS` (read from the 26.3 server classes).
pub const FLAGS_INDEX: u8 = 17;
/// Synched `Horse.DATA_ID_TYPE_VARIANT`.
pub const VARIANT_INDEX: u8 = 18;
/// Synched `Llama.DATA_STRENGTH_ID` and `Llama.DATA_VARIANT_ID`.
pub const STRENGTH_INDEX: u8 = 19;
pub const LLAMA_VARIANT_INDEX: u8 = 20;
const TAME: i8 = 2;
/// `AbstractHorse.getMaxTemper`.
const MAX_TEMPER: i32 = 100;
/// Temper gained by every ride that did not tame (`RunAroundLikeCrazyGoal`).
const TEMPER_STEP: i32 = 5;
/// `RunAroundLikeCrazyGoal` rolls `nextInt(adjustedTickDelay(50))`. The goal does not require an
/// update every tick, so `Goal.reducedTickDelay` halves it: `ceil(50 / 2) = 25`.
const BUCK_ROLL: i32 = 25;
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

/// Hangs a chest on a tame donkey or mule. `false` when it does not apply.
pub fn add_chest(store: &mut EntityStore, key: EntityKey) -> bool {
    match mob::state_mut(store, key) {
        Some(s)
            if matches!(
                s.kind,
                MobKind::Donkey | MobKind::Mule | MobKind::Llama | MobKind::TraderLlama
            ) && s.owner.is_some()
                && !s.chest =>
        {
            s.chest = true;
        }
        _ => return false,
    }
    sync(store, key);
    true
}

/// Whether `item` is what this animal wears on its body: horse armour for a horse, a carpet
/// for a llama.
pub fn wears(store: &EntityStore, key: EntityKey, item: &str) -> bool {
    match mob::state(store, key).map(|s| s.kind) {
        Some(MobKind::Horse) => is_armor(item),
        Some(MobKind::Llama | MobKind::TraderLlama) => item.ends_with("_carpet"),
        _ => false,
    }
}

/// How many chest columns the animal's inventory screen has: 5 for a donkey or mule with a chest,
/// the llama's strength for a llama with one, none otherwise.
pub fn chest_columns(store: &EntityStore, key: EntityKey) -> u8 {
    let Some(s) = mob::state(store, key) else {
        return 0;
    };
    if !s.chest {
        return 0;
    }
    match s.kind {
        MobKind::Donkey | MobKind::Mule => 5,
        MobKind::Llama | MobKind::TraderLlama => {
            match store.data(key).and_then(|d| d.metadata.get(STRENGTH_INDEX)) {
                Some(MetaValue::Int(strength)) => (*strength).clamp(1, 5) as u8,
                _ => 3,
            }
        }
        _ => 0,
    }
}

/// Sets what a tame animal wears from its inventory screen: the saddle (not a llama's) and the
/// body armour item id (0 for none) with its armour points. `true` when anything changed.
pub fn set_gear(
    store: &mut EntityStore,
    key: EntityKey,
    saddled: bool,
    armor: u16,
    points: f32,
) -> bool {
    let changed = match mob::state_mut(store, key) {
        Some(s) if s.kind.is_equine() && s.owner.is_some() => {
            let saddled = saddled && !s.kind.is_llama();
            let changed = s.saddled != saddled || s.armor != armor;
            s.saddled = saddled;
            s.armor = armor;
            s.armor_points = if armor == 0 { 0.0 } else { points };
            changed
        }
        _ => false,
    };
    if changed {
        sync(store, key);
    }
    changed
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
        Some(s)
            if matches!(
                s.kind,
                MobKind::Horse | MobKind::Llama | MobKind::TraderLlama
            ) && s.owner.is_some()
                && s.armor == 0 =>
        {
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
    if state.kind.is_llama() {
        metadata.define(STRENGTH_INDEX, MetaValue::Int(1));
        metadata.define(LLAMA_VARIANT_INDEX, MetaValue::Int(0));
    }
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let flags = if state.owner.is_some() { TAME } else { 0 };
    metadata.set(FLAGS_INDEX, MetaValue::Byte(flags));
    if state.kind != MobKind::Horse {
        metadata.set(VARIANT_INDEX, MetaValue::Boolean(state.chest));
    }
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
    mob::state(store, key)
        .is_some_and(|s| s.kind.is_equine() && !s.kind.is_llama() && s.owner.is_some() && s.saddled)
}

/// Puts a saddle on a tame, unsaddled horse. `false` when it does not apply.
pub fn saddle(store: &mut EntityStore, key: EntityKey) -> bool {
    match mob::state_mut(store, key) {
        Some(s) if s.kind.is_equine() && !s.kind.is_llama() && s.owner.is_some() && !s.saddled => {
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

/// `Mob.serverAiStep` runs goal selectors in full only when `(tickCount + id)` is even; on other
/// ticks it only updates goals that require an update every tick. `RunAroundLikeCrazyGoal` does
/// not override `requiresUpdateEveryTick`, so its roll lands once every two game ticks.
fn buck_goal_tick(tick_count: u32, id: i32) -> bool {
    tick_count.wrapping_add(id as u32).is_multiple_of(2)
}

/// `RunAroundLikeCrazyGoal`: on every goal phase tick, a wild equine with a rider has a 1-in-25
/// chance to compare its temper with the taming threshold (`nextInt(getMaxTemper()) < temper`);
/// a hit tames it, a miss gains 5 temper and ejects the rider. `rider` is `None` once the horse
/// is dismounted, which stops the goal without touching temper or ownership.
pub fn tick_rider(
    store: &mut EntityStore,
    key: EntityKey,
    rider: Option<Uuid>,
    rng: &mut Rng,
) -> Bucked {
    let Some(rider) = rider else {
        return Bucked::No;
    };
    let Some((tick_count, id)) = store.data(key).map(|data| (data.tick_count, data.id)) else {
        return Bucked::No;
    };
    if !buck_goal_tick(tick_count, id) {
        return Bucked::No;
    }
    let Some(state) = mob::state_mut(store, key) else {
        return Bucked::No;
    };
    if !state.kind.is_equine() || state.owner.is_some() || rng.next_int(BUCK_ROLL) != 0 {
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
            if tick_rider(&mut store, key, Some(rider), &mut rng) == Bucked::Tamed {
                tamed = true;
                break;
            }
            store.data_mut(key).unwrap().tick_count += 1;
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

    /// A seed whose first `rides` buck rolls hit and whose temper rolls all fail against the
    /// temper gained so far, consuming the rolls exactly like `tick_rider`.
    fn seed_that_bucks(rides: usize) -> u64 {
        (1..=1_000_000)
            .find(|&seed| {
                let mut rng = Rng::new(seed);
                (0..rides).all(|ride| {
                    if rng.next_int(BUCK_ROLL) != 0 {
                        return false;
                    }
                    rng.next_int(MAX_TEMPER) >= TEMPER_STEP * ride as i32
                })
            })
            .expect("a seed that hits the buck roll")
    }

    /// Moves `tick_count` to a game tick where `Mob.serverAiStep` runs the goal selector.
    fn goal_phase(store: &mut EntityStore, key: EntityKey) {
        let id = store.data(key).unwrap().id;
        store.data_mut(key).unwrap().tick_count = id as u32 & 1;
    }

    #[test]
    fn buck_attempts_follow_the_two_tick_goal_phase() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        let rider = Uuid::from_u128(5);
        let mut rng = Rng::new(seed_that_bucks(1));
        let id = store.data(key).unwrap().id;
        // On the off tick `serverAiStep` only updates every-tick goals, so no roll happens.
        store.data_mut(key).unwrap().tick_count = (id as u32 & 1) ^ 1;
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::No
        );
        assert_eq!(mob::state(&store, key).unwrap().temper, 0);
        // The next game tick is the goal phase and the first buck roll hits.
        store.data_mut(key).unwrap().tick_count += 1;
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::Wild
        );
        assert_eq!(mob::state(&store, key).unwrap().temper, TEMPER_STEP);
    }

    #[test]
    fn bucking_continues_on_later_goal_ticks() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        let rider = Uuid::from_u128(5);
        goal_phase(&mut store, key);
        let mut rng = Rng::new(seed_that_bucks(2));
        // The first ride throws the player; the test remounts for the next goal phase.
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::Wild
        );
        assert_eq!(mob::state(&store, key).unwrap().temper, TEMPER_STEP);
        store.data_mut(key).unwrap().tick_count += 2;
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::Wild
        );
        assert_eq!(mob::state(&store, key).unwrap().temper, TEMPER_STEP * 2);
    }

    #[test]
    fn a_tamed_horse_stops_bucking() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        let rider = Uuid::from_u128(5);
        goal_phase(&mut store, key);
        mob::state_mut(&mut store, key).unwrap().temper = MAX_TEMPER;
        let mut rng = Rng::new(seed_that_bucks(1));
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::Tamed
        );
        assert_eq!(mob::state(&store, key).unwrap().owner, Some(rider));
        for _ in 0..20 {
            store.data_mut(key).unwrap().tick_count += 1;
            assert_eq!(
                tick_rider(&mut store, key, Some(rider), &mut rng),
                Bucked::No
            );
        }
        assert_eq!(mob::state(&store, key).unwrap().owner, Some(rider));
        assert_eq!(mob::state(&store, key).unwrap().temper, MAX_TEMPER);
    }

    #[test]
    fn a_dismounted_wild_horse_never_rolls() {
        let mut store = EntityStore::default();
        let key = horse(&mut store);
        goal_phase(&mut store, key);
        let rider = Uuid::from_u128(5);
        let mut rng = Rng::new(seed_that_bucks(1));
        assert_eq!(tick_rider(&mut store, key, None, &mut rng), Bucked::No);
        assert_eq!(mob::state(&store, key).unwrap().temper, 0);
        // A buck throws the rider; the following tick has no passenger and cannot roll again.
        assert_eq!(
            tick_rider(&mut store, key, Some(rider), &mut rng),
            Bucked::Wild
        );
        let temper = mob::state(&store, key).unwrap().temper;
        store.data_mut(key).unwrap().tick_count += 2;
        assert_eq!(tick_rider(&mut store, key, None, &mut rng), Bucked::No);
        assert_eq!(mob::state(&store, key).unwrap().temper, temper);
    }

    #[test]
    fn only_a_tame_donkey_or_mule_takes_a_chest() {
        let mut store = EntityStore::default();
        let donkey = store.insert(new_mob(
            MobKind::Donkey,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        assert!(!add_chest(&mut store, donkey), "a wild donkey takes none");
        mob::state_mut(&mut store, donkey).unwrap().owner = Some(Uuid::from_u128(1));
        assert!(add_chest(&mut store, donkey));
        assert!(!add_chest(&mut store, donkey));
        assert_eq!(
            store.data(donkey).unwrap().metadata.get(VARIANT_INDEX),
            Some(&MetaValue::Boolean(true))
        );
        let horse = horse(&mut store);
        mob::state_mut(&mut store, horse).unwrap().owner = Some(Uuid::from_u128(1));
        assert!(!add_chest(&mut store, horse), "horses carry no chest");
    }

    #[test]
    fn llama_pack_columns_follow_strength_metadata() {
        let mut store = EntityStore::default();
        let llama = store.insert(new_mob(
            MobKind::Llama,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let state = mob::state_mut(&mut store, llama).unwrap();
        state.owner = Some(Uuid::from_u128(3));
        state.chest = true;

        for strength in 1..=5 {
            store
                .data_mut(llama)
                .unwrap()
                .metadata
                .set(STRENGTH_INDEX, MetaValue::Int(strength));
            assert_eq!(chest_columns(&store, llama), strength as u8);
        }

        mob::state_mut(&mut store, llama).unwrap().chest = false;
        assert_eq!(chest_columns(&store, llama), 0);
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
