//! Animal breeding (vanilla `Animal` / `AgeableMob`): feeding puts an adult in love, two animals
//! in love walk to each other and, after a short wait, produce a baby that grows up over 20 minutes.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `AgeableMob.DATA_BABY_ID` (read from the 26.3 server classes).
pub const BABY_INDEX: u8 = 16;
/// `AgeableMob.BABY_START_AGE`.
pub const BABY_AGE: i32 = -24_000;
/// Breeding cooldown the parents get (`Animal.finalizeSpawnChildFromBreeding`).
pub const PARENT_COOLDOWN: i32 = 6_000;
/// `Animal.setInLove`.
pub const LOVE_TICKS: u16 = 600;
/// How far an animal in love looks for a mate (`BreedGoal`).
const MATE_RANGE: f64 = 8.0;
/// `BreedGoal` breeds when the animals are within 3 blocks and have been for this long.
const BREED_DISTANCE_SQ: f64 = 9.0;
const BREED_TICKS: u16 = 60;

impl MobKind {
    /// The animals that breed: the farm animals and wolves.
    pub fn is_animal(self) -> bool {
        Self::ALL.contains(&self) || self.is_tameable()
    }
}

/// Vanilla `Animal.isFood`: the `cow_food`, `chicken_food`, `pig_food` and sheep (wheat) lists.
pub fn is_food(kind: MobKind, item: &str) -> bool {
    let item = item.strip_prefix("minecraft:").unwrap_or(item);
    match kind {
        MobKind::Cow | MobKind::Sheep => item == "wheat",
        MobKind::Pig => matches!(item, "carrot" | "potato" | "beetroot"),
        MobKind::Chicken => matches!(
            item,
            "wheat_seeds"
                | "beetroot_seeds"
                | "melon_seeds"
                | "pumpkin_seeds"
                | "torchflower_seeds"
                | "pitcher_pod"
        ),
        MobKind::Wolf => crate::tame::is_wolf_food(item),
        MobKind::Cat => crate::tame::is_cat_food(item),
        _ => false,
    }
}

/// Defines the baby flag on an animal's synched data.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(BABY_INDEX, MetaValue::Boolean(false));
    metadata.set(BABY_INDEX, MetaValue::Boolean(state.age < 0));
}

/// Babies are half the size of their parents (`AgeableMob.getAgeScale`).
const BABY_SCALE: f64 = 0.5;

fn sync(store: &mut EntityStore, key: EntityKey) {
    let Some(data) = store.data_mut(key) else {
        return;
    };
    let Some(mob) = data.mob else { return };
    data.metadata
        .set(BABY_INDEX, MetaValue::Boolean(mob.age < 0));
    let scale = if mob.age < 0 { BABY_SCALE } else { 1.0 };
    let props = mob.kind.props();
    let (width, height) = (
        f64::from(props.width) * scale,
        f64::from(props.height) * scale,
    );
    if data.width != width || data.height != height {
        data.width = width;
        data.height = height;
        if let Some(pos) = store.pos(key) {
            store.set_pos(key, pos);
        }
    }
}

/// What feeding an animal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fed {
    /// Not food for it, or it cannot take it now; the item is not used up.
    No,
    /// An adult fell in love.
    Love,
    /// A baby grew up faster.
    Grew,
}

/// Vanilla `Animal.mobInteract` with food in hand.
pub fn feed(store: &mut EntityStore, key: EntityKey, item: &str, player: Uuid) -> Fed {
    let Some(mob) = mob::state_mut(store, key) else {
        return Fed::No;
    };
    if !is_food(mob.kind, item) {
        return Fed::No;
    }
    // A wild wolf or cat is not fed; only a tamed one breeds.
    if mob.kind.is_tameable() && mob.owner.is_none() {
        return Fed::No;
    }
    let fed = if mob.age == 0 && mob.love == 0 {
        mob.love = LOVE_TICKS;
        mob.love_cause = Some(player);
        Fed::Love
    } else if mob.age < 0 {
        let seconds = (-mob.age as f32 / 20.0 * 0.1) as i32;
        mob.age = (mob.age + seconds * 20).min(0);
        Fed::Grew
    } else {
        Fed::No
    };
    sync(store, key);
    fed
}

/// `AgeableMob.aiStep` and the love timer, once per tick.
pub fn tick_age(store: &mut EntityStore, key: EntityKey) {
    let Some(mob) = mob::state_mut(store, key) else {
        return;
    };
    if mob.age < 0 {
        mob.age += 1;
    } else if mob.age > 0 {
        mob.age -= 1;
    }
    mob.love = mob.love.saturating_sub(1);
    if mob.love == 0 {
        mob.love_cause = None;
        mob.breed_timer = 0;
    }
    sync(store, key);
}

/// The nearest other animal of the same kind that is in love too, for an animal in love.
pub fn mate_for(store: &EntityStore, key: EntityKey) -> Option<(EntityKey, Vec3)> {
    let me = mob::state(store, key).copied()?;
    if me.love == 0 || me.age != 0 {
        return None;
    }
    let from = store.pos(key)?;
    let mut best: Option<(f64, EntityKey, Vec3)> = None;
    for &other in store.keys() {
        if other == key {
            continue;
        }
        let Some(state) = mob::state(store, other) else {
            continue;
        };
        if state.kind != me.kind || state.love == 0 || state.age != 0 || state.health <= 0.0 {
            continue;
        }
        let Some(pos) = store.pos(other) else {
            continue;
        };
        let distance = (pos.x - from.x).powi(2) + (pos.z - from.z).powi(2);
        if distance <= MATE_RANGE * MATE_RANGE && best.is_none_or(|(d, ..)| distance < d) {
            best = Some((distance, other, pos));
        }
    }
    best.map(|(_, other, pos)| (other, pos))
}

/// Two animals bred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bred {
    /// The kind of the baby.
    pub kind: MobKind,
    /// The player who fed the first parent.
    pub cause: Option<Uuid>,
    pub pos: Vec3,
}

/// Counts the time the animal has stood next to its mate and breeds after [`BREED_TICKS`].
/// Returns the baby's spawn data when it happened.
pub fn approach(
    store: &mut EntityStore,
    key: EntityKey,
    mate: EntityKey,
    mate_pos: Vec3,
    rng: &mut Rng,
) -> Option<Bred> {
    let from = store.pos(key)?;
    let close = (mate_pos.x - from.x).powi(2) + (mate_pos.z - from.z).powi(2) < BREED_DISTANCE_SQ;
    let state = mob::state_mut(store, key)?;
    if !close {
        state.breed_timer = 0;
        return None;
    }
    state.breed_timer += 1;
    if state.breed_timer < BREED_TICKS {
        return None;
    }
    let kind = state.kind;
    let cause = state.love_cause;
    for parent in [key, mate] {
        if let Some(state) = mob::state_mut(store, parent) {
            state.age = PARENT_COOLDOWN;
            state.love = 0;
            state.love_cause = None;
            state.breed_timer = 0;
        }
        sync(store, parent);
    }
    let pos = Vec3::new(
        (from.x + mate_pos.x) / 2.0,
        from.y,
        (from.z + mate_pos.z) / 2.0,
    );
    let mut baby = mob::new_mob(kind, pos, rng);
    if let Some(state) = baby.data.mob.as_mut() {
        state.age = BABY_AGE;
    }
    if let Some(state) = baby.data.mob {
        define_metadata(&mut baby.data.metadata, &state);
    }
    store.insert(baby);
    Some(Bred { kind, cause, pos })
}

/// A thrown egg that lands may hatch (vanilla `ThrownEgg.onHit`): one time in eight, a baby
/// chicken, and one time in 32 of those four of them. Returns how many chicks appeared.
pub fn hatch_egg(store: &mut EntityStore, pos: Vec3, rng: &mut Rng) -> usize {
    if rng.next_int(8) != 0 {
        return 0;
    }
    let count = if rng.next_int(32) == 0 { 4 } else { 1 };
    for _ in 0..count {
        let mut chick = mob::new_mob(MobKind::Chicken, pos, rng);
        if let Some(state) = chick.data.mob.as_mut() {
            state.age = BABY_AGE;
        }
        if let Some(state) = chick.data.mob {
            define_metadata(&mut chick.data.metadata, &state);
        }
        store.insert(chick);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn some_landed_eggs_hatch_baby_chickens() {
        let mut rng = Rng::new(11);
        let mut store = EntityStore::default();
        let mut chicks = 0;
        for _ in 0..400 {
            chicks += hatch_egg(&mut store, Vec3::new(0.5, 64.0, 0.5), &mut rng);
        }
        // About 1/8 of 400 eggs hatch.
        assert!((20..120).contains(&chicks), "{chicks}");
        let key = store.keys()[0];
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.kind, MobKind::Chicken);
        assert_eq!(state.age, BABY_AGE);
    }

    fn pair(store: &mut EntityStore, rng: &mut Rng) -> (EntityKey, EntityKey) {
        let a = store.insert(mob::new_mob(MobKind::Cow, Vec3::new(0.5, 64.0, 0.5), rng));
        let b = store.insert(mob::new_mob(MobKind::Cow, Vec3::new(1.5, 64.0, 0.5), rng));
        (a, b)
    }

    #[test]
    fn only_the_right_food_puts_an_adult_in_love_once() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let (cow, _) = pair(&mut store, &mut rng);
        let player = Uuid::from_u128(7);
        assert_eq!(feed(&mut store, cow, "minecraft:carrot", player), Fed::No);
        assert_eq!(feed(&mut store, cow, "minecraft:wheat", player), Fed::Love);
        assert_eq!(feed(&mut store, cow, "minecraft:wheat", player), Fed::No);
        assert_eq!(mob::state(&store, cow).unwrap().love, LOVE_TICKS);
    }

    #[test]
    fn a_baby_grows_faster_when_fed_and_shows_the_baby_flag() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let (cow, _) = pair(&mut store, &mut rng);
        mob::state_mut(&mut store, cow).unwrap().age = BABY_AGE;
        sync(&mut store, cow);
        assert_eq!(
            store.data(cow).unwrap().metadata.get(BABY_INDEX),
            Some(&MetaValue::Boolean(true))
        );
        let player = Uuid::from_u128(7);
        assert_eq!(feed(&mut store, cow, "minecraft:wheat", player), Fed::Grew);
        assert_eq!(mob::state(&store, cow).unwrap().age, BABY_AGE + 2400);
    }

    #[test]
    fn two_animals_in_love_breed_a_baby_and_get_a_cooldown() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let (a, b) = pair(&mut store, &mut rng);
        let player = Uuid::from_u128(7);
        assert_eq!(mate_for(&store, a), None);
        feed(&mut store, a, "minecraft:wheat", player);
        assert_eq!(mate_for(&store, a), None);
        feed(&mut store, b, "minecraft:wheat", Uuid::from_u128(8));
        let (mate, mate_pos) = mate_for(&store, a).unwrap();
        assert_eq!(mate, b);
        let mut bred = None;
        for _ in 0..BREED_TICKS {
            bred = approach(&mut store, a, mate, mate_pos, &mut rng);
        }
        let bred = bred.expect("bred after the wait");
        assert_eq!(bred.cause, Some(player));
        assert_eq!(store.keys().len(), 3);
        for parent in [a, b] {
            let state = mob::state(&store, parent).unwrap();
            assert_eq!((state.age, state.love), (PARENT_COOLDOWN, 0));
        }
        let baby = store.keys()[2];
        let state = mob::state(&store, baby).unwrap();
        assert_eq!(state.age, BABY_AGE);
        assert_eq!(
            store.data(baby).unwrap().metadata.get(BABY_INDEX),
            Some(&MetaValue::Boolean(true))
        );
    }

    #[test]
    fn a_baby_is_half_the_size_and_grows_to_full_size() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let (cow, _) = pair(&mut store, &mut rng);
        let full = store.bbox(cow).unwrap();
        mob::state_mut(&mut store, cow).unwrap().age = BABY_AGE;
        tick_age(&mut store, cow);
        let baby = store.bbox(cow).unwrap();
        assert!(((baby.max.y - baby.min.y) * 2.0 - (full.max.y - full.min.y)).abs() < 1e-9);
        mob::state_mut(&mut store, cow).unwrap().age = -1;
        tick_age(&mut store, cow);
        assert_eq!(store.bbox(cow).unwrap(), full);
    }

    #[test]
    fn only_a_tamed_wolf_takes_meat() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let wolf = store.insert(mob::new_mob(
            MobKind::Wolf,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let player = Uuid::from_u128(7);
        assert_eq!(feed(&mut store, wolf, "minecraft:beef", player), Fed::No);
        mob::state_mut(&mut store, wolf).unwrap().owner = Some(player);
        assert_eq!(feed(&mut store, wolf, "minecraft:beef", player), Fed::Love);
    }

    #[test]
    fn age_counts_towards_zero_and_the_flag_clears() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let (cow, _) = pair(&mut store, &mut rng);
        mob::state_mut(&mut store, cow).unwrap().age = -2;
        tick_age(&mut store, cow);
        tick_age(&mut store, cow);
        assert_eq!(mob::state(&store, cow).unwrap().age, 0);
        assert_eq!(
            store.data(cow).unwrap().metadata.get(BABY_INDEX),
            Some(&MetaValue::Boolean(false))
        );
    }
}
