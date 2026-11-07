//! Taming (vanilla `TamableAnimal` / `Wolf`): a bone tames a wolf one time in three, and the
//! owner can then make it sit or stand. A tamed wolf that is standing follows its owner.

use crate::mob::{self, MobKind, MobState};
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
use uuid::Uuid;

/// Synched `TamableAnimal.DATA_FLAGS_ID` (read from the 26.3 server classes).
pub const FLAGS_INDEX: u8 = 18;
/// Synched `TamableAnimal.DATA_OWNERUUID_ID`.
pub const OWNER_INDEX: u8 = 19;
const SITTING: i8 = 1;
const TAMED: i8 = 4;
/// A tamed wolf has 40 maximum health (`Wolf.setTame`).
const TAMED_HEALTH: f32 = 40.0;
/// `FollowOwnerGoal`: start following farther than this, stop once closer than `STOP_DISTANCE`.
const START_DISTANCE_SQ: f64 = 100.0;
const STOP_DISTANCE_SQ: f64 = 4.0;
/// Farther than this the wolf teleports to its owner (`FollowOwnerGoal.teleportToOwner`).
const TELEPORT_DISTANCE_SQ: f64 = 144.0;

/// Defines the tamable data values on a wolf and writes the current state into them.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(FLAGS_INDEX, MetaValue::Byte(0));
    metadata.define(OWNER_INDEX, MetaValue::OptionalUuid(None));
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let mut flags = 0;
    if state.sitting {
        flags |= SITTING;
    }
    if state.owner.is_some() {
        flags |= TAMED;
    }
    metadata.set(FLAGS_INDEX, MetaValue::Byte(flags));
    metadata.set(OWNER_INDEX, MetaValue::OptionalUuid(state.owner));
}

fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
    {
        write_metadata(&mut data.metadata, &state);
    }
}

/// What a right-click on a tamable animal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    /// Nothing; the item is not used up.
    No,
    /// A taming item was used and the animal stayed wild.
    Consumed,
    /// A taming item was used and the animal is tame now.
    Tamed,
    /// The owner made the animal sit or stand.
    Toggled,
}

impl Used {
    /// The vanilla entity event the client needs after this use: 7 hearts when taming worked and
    /// 6 smoke when it failed. Sitting and ordinary uses have no event.
    pub fn status(self) -> Option<u8> {
        match self {
            Self::Tamed => Some(7),
            Self::Consumed => Some(6),
            Self::No | Self::Toggled => None,
        }
    }
}

/// Meat, which tamed wolves eat (`#minecraft:wolf_food`).
pub fn is_wolf_food(item: &str) -> bool {
    matches!(
        item.strip_prefix("minecraft:").unwrap_or(item),
        "beef"
            | "cooked_beef"
            | "porkchop"
            | "cooked_porkchop"
            | "chicken"
            | "cooked_chicken"
            | "mutton"
            | "cooked_mutton"
            | "rabbit"
            | "cooked_rabbit"
            | "rotten_flesh"
    )
}

/// Raw fish, which tames cats and which tamed cats breed with (`#minecraft:cat_food`).
pub fn is_cat_food(item: &str) -> bool {
    matches!(
        item.strip_prefix("minecraft:").unwrap_or(item),
        "cod" | "salmon"
    )
}

/// Seeds, which tame parrots (`#minecraft:parrot_food`).
pub fn is_parrot_food(item: &str) -> bool {
    matches!(
        item.strip_prefix("minecraft:").unwrap_or(item),
        "wheat_seeds"
            | "melon_seeds"
            | "pumpkin_seeds"
            | "beetroot_seeds"
            | "torchflower_seeds"
            | "pitcher_pod"
    )
}

/// Vanilla `Wolf.mobInteract` / `Cat.mobInteract` / `Parrot.mobInteract`, taming and sitting only.
pub fn use_on(
    store: &mut EntityStore,
    key: EntityKey,
    item: &str,
    player: Uuid,
    rng: &mut Rng,
) -> Used {
    let Some(state) = mob::state_mut(store, key) else {
        return Used::No;
    };
    if !state.kind.is_tameable() {
        return Used::No;
    }
    let (tame_health, tames, food) = match state.kind {
        // `Cat.setTame` and `Parrot.setTame` restore the entity's data-driven maximum health.
        MobKind::Cat => (
            MobKind::Cat.max_health(),
            is_cat_food(item),
            is_cat_food(item),
        ),
        MobKind::Parrot => (
            MobKind::Parrot.max_health(),
            is_parrot_food(item),
            is_parrot_food(item),
        ),
        _ => (TAMED_HEALTH, item == "minecraft:bone", is_wolf_food(item)),
    };
    let used = match state.owner {
        None if tames => {
            if rng.next_int(3) == 0 {
                state.owner = Some(player);
                state.sitting = true;
                state.health = state.health.max(tame_health);
                Used::Tamed
            } else {
                Used::Consumed
            }
        }
        Some(owner) if owner == player && !food => {
            state.sitting = !state.sitting;
            state.following = false;
            Used::Toggled
        }
        _ => Used::No,
    };
    sync(store, key);
    used
}

/// Where a tamed, standing wolf should walk to follow its owner, if it should. Far away it jumps
/// next to the owner instead.
pub fn follow_target(
    store: &mut EntityStore,
    key: EntityKey,
    owners: &[(Uuid, Vec3)],
    from: Vec3,
) -> Option<Vec3> {
    let state = mob::state(store, key).copied()?;
    let owner = state.owner?;
    if state.sitting {
        return None;
    }
    let (_, at) = owners.iter().find(|(uuid, _)| *uuid == owner)?;
    let distance = (at.x - from.x).powi(2) + (at.z - from.z).powi(2);
    let following = state.following && distance > STOP_DISTANCE_SQ || distance > START_DISTANCE_SQ;
    if let Some(state) = mob::state_mut(store, key) {
        state.following = following;
    }
    if distance >= TELEPORT_DISTANCE_SQ {
        store.set_pos(key, Vec3::new(at.x + 1.0, at.y, at.z));
        return None;
    }
    following.then_some(*at)
}

/// Damage a tamed wolf bites for (`Wolf.setTame`: 4 attack damage once tamed).
pub const TAMED_BITE: f32 = 4.0;
/// A tamed wolf only defends an owner this close (`OwnerHurtByTargetGoal` target range).
const DEFEND_RANGE_SQ: f64 = 16.0 * 16.0;

/// The hostile mob a tamed wolf should fight after its owner attacked it, falling back to a mob
/// that is already targeting the owner. `attacks` contains the same-tick player attack targets.
/// Like the owner-defense goal, a direct target is ignored when either it or the owner is more
/// than 16 blocks from the wolf.
pub fn defend_target_with_owner_attack(
    store: &EntityStore,
    key: EntityKey,
    owners: &[(Uuid, Vec3)],
    attacks: &[(Uuid, EntityKey)],
    from: Vec3,
) -> Option<(EntityKey, Vec3)> {
    let state = mob::state(store, key)?;
    let owner = state.owner?;
    if state.sitting {
        return None;
    }
    let (_, owner_pos) = owners.iter().find(|(uuid, _)| *uuid == owner)?;
    let near = |p: Vec3, o: Vec3| (p.x - o.x).powi(2) + (p.z - o.z).powi(2) <= DEFEND_RANGE_SQ;
    if !near(*owner_pos, from) {
        return None;
    }

    if let Some((_, attacked)) = attacks.iter().find(|(uuid, _)| *uuid == owner)
        && *attacked != key
        && mob::is_mob(store, *attacked)
        && mob::state(store, *attacked).is_some_and(|mob| !mob.is_dead())
        && let Some(pos) = store.pos(*attacked)
        && near(pos, from)
    {
        return Some((*attacked, pos));
    }

    defend_target(store, key, owners, from)
}

/// The nearest hostile mob targeting a tamed wolf's owner while both are within 16 blocks
/// (`OwnerHurtByTargetGoal` fallback). The owner's same-tick direct hits are handled by
/// [`defend_target_with_owner_attack`].
pub fn defend_target(
    store: &EntityStore,
    key: EntityKey,
    owners: &[(Uuid, Vec3)],
    from: Vec3,
) -> Option<(EntityKey, Vec3)> {
    let state = mob::state(store, key)?;
    let owner = state.owner?;
    if state.sitting {
        return None;
    }
    let (_, at) = owners.iter().find(|(uuid, _)| *uuid == owner)?;
    let near = |p: Vec3, o: Vec3| (p.x - o.x).powi(2) + (p.z - o.z).powi(2) <= DEFEND_RANGE_SQ;
    if !near(*at, from) {
        return None;
    }
    store
        .keys()
        .iter()
        .copied()
        .filter(|&other| other != key)
        .filter_map(|other| {
            let m = mob::state(store, other)?;
            if m.is_dead() || !m.kind.is_hostile() || m.target != Some(owner) {
                return None;
            }
            let pos = store.pos(other)?;
            near(pos, from).then_some((other, pos))
        })
        .min_by(|a, b| {
            let da = (a.1.x - from.x).powi(2) + (a.1.z - from.z).powi(2);
            let db = (b.1.x - from.x).powi(2) + (b.1.z - from.z).powi(2);
            da.total_cmp(&db)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wolf(store: &mut EntityStore, rng: &mut Rng) -> EntityKey {
        store.insert(mob::new_mob(MobKind::Wolf, Vec3::new(0.5, 64.0, 0.5), rng))
    }

    #[test]
    fn a_tamed_standing_wolf_picks_the_nearest_mob_hunting_its_owner() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(1);
        let wolf = wolf(&mut store, &mut rng);
        {
            let state = mob::state_mut(&mut store, wolf).unwrap();
            state.owner = Some(owner);
            state.sitting = false;
        }
        let near = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        let far = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(9.5, 64.0, 0.5),
            &mut rng,
        ));
        let idle = store.insert(mob::new_mob(
            MobKind::Zombie,
            Vec3::new(2.5, 64.0, 0.5),
            &mut rng,
        ));
        for key in [near, far] {
            mob::state_mut(&mut store, key).unwrap().target = Some(owner);
        }
        let owners = [(owner, Vec3::new(0.5, 64.0, 0.5))];
        let from = Vec3::new(0.5, 64.0, 0.5);
        let _ = idle;
        assert_eq!(
            defend_target(&store, wolf, &owners, from).map(|t| t.0),
            Some(near)
        );
        mob::state_mut(&mut store, wolf).unwrap().sitting = true;
        assert!(defend_target(&store, wolf, &owners, from).is_none());
    }

    #[test]
    fn a_tamed_wolf_targets_a_mob_its_owner_attacked() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(1);
        let wolf = wolf(&mut store, &mut rng);
        mob::state_mut(&mut store, wolf).unwrap().owner = Some(owner);
        let attacked = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        let owners = [(owner, Vec3::new(0.5, 64.0, 0.5))];

        assert_eq!(
            defend_target_with_owner_attack(
                &store,
                wolf,
                &owners,
                &[(owner, attacked)],
                Vec3::new(0.5, 64.0, 0.5)
            ),
            Some((attacked, Vec3::new(4.5, 64.0, 0.5)))
        );
    }

    #[test]
    fn a_tamed_wolf_ignores_owner_attack_targets_outside_its_defend_range() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(1);
        let wolf = wolf(&mut store, &mut rng);
        mob::state_mut(&mut store, wolf).unwrap().owner = Some(owner);
        let attacked = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(20.5, 64.0, 0.5),
            &mut rng,
        ));
        let owners = [(owner, Vec3::new(19.5, 64.0, 0.5))];

        assert_eq!(
            defend_target_with_owner_attack(
                &store,
                wolf,
                &owners,
                &[(owner, attacked)],
                Vec3::new(0.5, 64.0, 0.5)
            ),
            None
        );
    }

    #[test]
    fn a_tamed_wolf_only_follows_its_own_owners_attack_and_not_while_sitting() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        let wolf = wolf(&mut store, &mut rng);
        {
            let state = mob::state_mut(&mut store, wolf).unwrap();
            state.owner = Some(owner);
            state.sitting = false;
        }
        let attacked = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(4.5, 64.0, 0.5),
            &mut rng,
        ));
        let owners = [(owner, Vec3::new(0.5, 64.0, 0.5))];
        let from = Vec3::new(0.5, 64.0, 0.5);

        assert_eq!(
            defend_target_with_owner_attack(&store, wolf, &owners, &[(other, attacked)], from),
            None
        );
        mob::state_mut(&mut store, wolf).unwrap().sitting = true;
        assert_eq!(
            defend_target_with_owner_attack(&store, wolf, &owners, &[(owner, attacked)], from),
            None
        );
    }

    #[test]
    fn seeds_tame_a_parrot_but_it_never_breeds() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Parrot,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let player = Uuid::from_u128(1);
        assert_eq!(
            crate::breed::feed(&mut store, key, "minecraft:wheat_seeds", player),
            crate::breed::Fed::No
        );
        let mut tries = 0;
        while use_on(&mut store, key, "minecraft:wheat_seeds", player, &mut rng) != Used::Tamed {
            tries += 1;
            assert!(tries < 100);
        }
        assert_eq!(mob::state(&store, key).unwrap().owner, Some(player));
        assert_eq!(
            crate::breed::feed(&mut store, key, "minecraft:wheat_seeds", player),
            crate::breed::Fed::No
        );
    }

    #[test]
    fn raw_fish_tames_a_cat_and_a_bone_does_not() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            MobKind::Cat,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let player = Uuid::from_u128(1);
        assert_eq!(
            use_on(&mut store, key, "minecraft:bone", player, &mut rng),
            Used::No
        );
        let mut tries = 0;
        while use_on(&mut store, key, "minecraft:salmon", player, &mut rng) != Used::Tamed {
            tries += 1;
            assert!(tries < 100);
        }
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.owner, Some(player));
        assert_eq!(state.health, MobKind::Cat.max_health());
        assert_eq!(MobKind::Cat.max_health(), 10.0);
        // Fish now feeds it instead of toggling sitting; anything else toggles.
        assert_eq!(
            use_on(&mut store, key, "minecraft:cod", player, &mut rng),
            Used::No
        );
        assert_eq!(
            use_on(&mut store, key, "minecraft:air", player, &mut rng),
            Used::Toggled
        );
    }

    #[test]
    fn failed_taming_uses_the_item_and_leaves_cats_and_parrots_wild() {
        for (kind, item) in [
            (MobKind::Cat, "minecraft:cod"),
            (MobKind::Parrot, "minecraft:wheat_seeds"),
        ] {
            let mut spawn_rng = Rng::new(3);
            let mut store = EntityStore::default();
            let key = store.insert(mob::new_mob(
                kind,
                Vec3::new(0.5, 64.0, 0.5),
                &mut spawn_rng,
            ));
            let player = Uuid::from_u128(1);
            // Find a seed that fails on its first roll, so the failure path is exact.
            let seed = (0u64..)
                .find(|&seed| {
                    let mut rng = Rng::new(seed);
                    rng.next_int(3) != 0
                })
                .unwrap();
            let mut rng = Rng::new(seed);
            let failed = use_on(&mut store, key, item, player, &mut rng);
            assert_eq!(failed, Used::Consumed);
            assert_eq!(failed.status(), Some(6));
            assert_eq!(mob::state(&store, key).unwrap().owner, None);

            let mut tries = 0;
            while use_on(&mut store, key, item, player, &mut rng) != Used::Tamed {
                tries += 1;
                assert!(tries < 100);
            }
            assert_eq!(Used::Tamed.status(), Some(7));
            let state = mob::state(&store, key).unwrap();
            assert_eq!(state.owner, Some(player));
            assert_eq!(state.health, kind.max_health());
            assert!(state.sitting);
        }
    }

    #[test]
    fn tamed_cats_and_parrots_sit_and_follow_only_their_owner() {
        for (kind, item) in [
            (MobKind::Cat, "minecraft:salmon"),
            (MobKind::Parrot, "minecraft:pumpkin_seeds"),
        ] {
            let mut rng = Rng::new(3);
            let mut store = EntityStore::default();
            let key = store.insert(mob::new_mob(kind, Vec3::new(0.5, 64.0, 0.5), &mut rng));
            let owner = Uuid::from_u128(1);
            let other = Uuid::from_u128(2);
            let mut tries = 0;
            while use_on(&mut store, key, item, owner, &mut rng) != Used::Tamed {
                tries += 1;
                assert!(tries < 100);
            }
            let from = store.pos(key).unwrap();
            assert!(mob::state(&store, key).unwrap().sitting);
            assert_eq!(mob::state(&store, key).unwrap().owner, Some(owner));
            let far = [(owner, Vec3::new(11.0, 64.0, 0.5))];
            assert_eq!(follow_target(&mut store, key, &far, from), None);

            // Only the owner can stand it up; another player has no effect.
            assert_eq!(
                use_on(&mut store, key, "minecraft:air", other, &mut rng),
                Used::No
            );
            assert!(mob::state(&store, key).unwrap().sitting);
            assert_eq!(
                use_on(&mut store, key, "minecraft:air", owner, &mut rng),
                Used::Toggled
            );
            assert!(!mob::state(&store, key).unwrap().sitting);
            assert_eq!(
                store.data(key).unwrap().metadata.get(FLAGS_INDEX),
                Some(&MetaValue::Byte(TAMED))
            );
            assert_eq!(
                store.data(key).unwrap().metadata.get(OWNER_INDEX),
                Some(&MetaValue::OptionalUuid(Some(owner)))
            );

            assert!(follow_target(&mut store, key, &far, from).is_some());
            let near = [(owner, Vec3::new(2.0, 64.0, 0.5))];
            assert_eq!(follow_target(&mut store, key, &near, near[0].1), None);
            let very_far = [(owner, Vec3::new(50.0, 64.0, 0.5))];
            assert_eq!(follow_target(&mut store, key, &very_far, from), None);
            assert!((store.pos(key).unwrap().x - 51.0).abs() < 1e-9);
        }
    }

    #[test]
    fn cat_breeding_needs_taming_and_fish_while_parrots_never_breed() {
        let owner = Uuid::from_u128(1);
        for (kind, food) in [
            (MobKind::Cat, "minecraft:cod"),
            (MobKind::Parrot, "minecraft:wheat_seeds"),
        ] {
            let mut rng = Rng::new(3);
            let mut store = EntityStore::default();
            let key = store.insert(mob::new_mob(kind, Vec3::new(0.5, 64.0, 0.5), &mut rng));
            assert_eq!(
                crate::breed::feed(&mut store, key, food, owner),
                crate::breed::Fed::No,
                "a wild {kind:?} is not fed"
            );
            let mut tries = 0;
            while use_on(&mut store, key, food, owner, &mut rng) != Used::Tamed {
                tries += 1;
                assert!(tries < 100);
            }
            let fed = crate::breed::feed(&mut store, key, food, owner);
            if kind == MobKind::Cat {
                assert_eq!(fed, crate::breed::Fed::Love);
                assert_eq!(
                    crate::breed::feed(&mut store, key, food, owner),
                    crate::breed::Fed::No,
                    "one fish starts one love timer"
                );
                let mate = store.insert(mob::new_mob(
                    MobKind::Cat,
                    Vec3::new(1.5, 64.0, 0.5),
                    &mut rng,
                ));
                mob::state_mut(&mut store, mate).unwrap().owner = Some(owner);
                assert_eq!(
                    crate::breed::feed(&mut store, mate, food, owner),
                    crate::breed::Fed::Love
                );
                assert_eq!(
                    crate::breed::mate_for(&store, key).map(|(mate, _)| mate),
                    Some(mate)
                );
            } else {
                assert_eq!(fed, crate::breed::Fed::No);
                assert_eq!(crate::breed::mate_for(&store, key), None);
            }
        }
    }

    #[test]
    fn taming_results_map_to_the_vanilla_entity_events() {
        assert_eq!(Used::Tamed.status(), Some(7));
        assert_eq!(Used::Consumed.status(), Some(6));
        assert_eq!(Used::Toggled.status(), None);
        assert_eq!(Used::No.status(), None);
    }

    #[test]
    fn a_bone_tames_a_wolf_sooner_or_later_and_other_items_do_nothing() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let player = Uuid::from_u128(1);
        assert_eq!(
            use_on(&mut store, key, "minecraft:stick", player, &mut rng),
            Used::No
        );
        let mut tries = 0;
        while use_on(&mut store, key, "minecraft:bone", player, &mut rng) != Used::Tamed {
            tries += 1;
            assert!(tries < 100);
        }
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.owner, Some(player));
        assert!(state.sitting);
        let meta = &store.data(key).unwrap().metadata;
        assert_eq!(
            meta.get(FLAGS_INDEX),
            Some(&MetaValue::Byte(SITTING | TAMED))
        );
        assert_eq!(
            meta.get(OWNER_INDEX),
            Some(&MetaValue::OptionalUuid(Some(player)))
        );
    }

    #[test]
    fn only_the_owner_toggles_sitting_and_food_does_not() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let owner = Uuid::from_u128(1);
        let state = mob::state_mut(&mut store, key).unwrap();
        state.owner = Some(owner);
        state.sitting = true;
        let other = Uuid::from_u128(2);
        assert_eq!(
            use_on(&mut store, key, "minecraft:air", other, &mut rng),
            Used::No
        );
        assert_eq!(
            use_on(&mut store, key, "minecraft:beef", owner, &mut rng),
            Used::No
        );
        assert_eq!(
            use_on(&mut store, key, "minecraft:air", owner, &mut rng),
            Used::Toggled
        );
        assert!(!mob::state(&store, key).unwrap().sitting);
    }

    #[test]
    fn a_standing_tamed_wolf_follows_far_owners_and_teleports_to_very_far_ones() {
        let mut rng = Rng::new(3);
        let mut store = EntityStore::default();
        let key = wolf(&mut store, &mut rng);
        let owner = Uuid::from_u128(1);
        mob::state_mut(&mut store, key).unwrap().owner = Some(owner);
        let from = store.pos(key).unwrap();
        let near = [(owner, Vec3::new(3.0, 64.0, 0.5))];
        assert_eq!(follow_target(&mut store, key, &near, from), None);
        let far = [(owner, Vec3::new(11.0, 64.0, 0.5))];
        assert!(follow_target(&mut store, key, &far, from).is_some());
        // Once following it keeps going until it is close.
        let middle = [(owner, Vec3::new(5.0, 64.0, 0.5))];
        assert!(follow_target(&mut store, key, &middle, from).is_some());
        let very_far = [(owner, Vec3::new(50.0, 64.0, 0.5))];
        assert_eq!(follow_target(&mut store, key, &very_far, from), None);
        assert!((store.pos(key).unwrap().x - 51.0).abs() < 1e-9);
        mob::state_mut(&mut store, key).unwrap().sitting = true;
        assert_eq!(follow_target(&mut store, key, &far, from), None);
    }
}
