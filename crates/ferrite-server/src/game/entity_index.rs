//! A world-visible snapshot of the region entity stores.
//!
//! Folia keeps the authoritative entity list inside each region. Commands run on the world
//! loop, so each region publishes the entities it owns into this index at the end of its tick.
//! Commands only read the snapshot: a selector is resolved here, then the mutation (kill,
//! teleport) is scheduled on the chunk that owns the entity. The region store stays the only
//! authority and the region hand-off still moves an entity between regions by value.

use ferrite_entity::{EntityStore, Flags};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

/// One entity as entity selectors see it. `tags` are the entity's `/tag` names.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandEntity {
    pub uuid: Uuid,
    pub network_id: i32,
    /// Entity type protocol id (`ferrite_data::entity_props`).
    pub kind: u16,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// The dimension whose region published this snapshot entry.
    pub dimension: super::Dimension,
    pub tags: BTreeSet<String>,
    /// Custom name from NBT/metadata, when the entity has one.
    pub custom_name: Option<String>,
    /// Current health of a mob; players' health lives in their region, not in this view.
    pub health: Option<f32>,
    /// The last entity that hurt this one (vanilla `LivingEntity.getLastHurtByMob`). The region
    /// records the latest attacker for kill credit, which is the same relation `/execute on
    /// attacker` asks for.
    pub attacker: Option<Uuid>,
    /// Owner of a tamed animal (`OwnableEntity.getOwner` for `/execute on owner`).
    pub owner: Option<Uuid>,
    /// The entity holding this one's lead (`Leashable.getLeashHolder` for `on leasher`).
    pub leasher: Option<Uuid>,
    /// The mob's current attack target (`Mob.getTarget` for `on target`).
    pub target: Option<Uuid>,
}

impl CommandEntity {
    /// Vanilla entity type id, e.g. `minecraft:zombie`.
    pub fn type_name(&self) -> &'static str {
        ferrite_data::entity_props::get(self.kind).key
    }

    /// The name `/kill` and selector feedback use.
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.custom_name {
            return name.clone();
        }
        let key = self.type_name();
        let path = key.strip_prefix("minecraft:").unwrap_or(key);
        let mut chars = path.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => key.to_owned(),
        }
    }
}

/// The command-visible entity snapshot, keyed by UUID.
#[derive(Debug, Default)]
pub struct EntityIndex {
    entries: BTreeMap<Uuid, CommandEntity>,
    /// Who created each entity (`TraceableEntity.getOwner` for `/execute on origin`): the shooter
    /// of an arrow or trident, the thrower of an item, the owner of a fishing bobber. Kept beside
    /// `entries` so `CommandEntity` literals elsewhere stay unchanged.
    origins: BTreeMap<Uuid, Uuid>,
}

impl EntityIndex {
    pub fn entries(&self) -> impl Iterator<Item = &CommandEntity> {
        self.entries.values()
    }

    pub fn get(&self, uuid: Uuid) -> Option<&CommandEntity> {
        self.entries.get(&uuid)
    }

    /// Records who created `entity` (used by `sync` and by tests).
    pub fn set_origin(&mut self, entity: Uuid, origin: Uuid) {
        self.origins.insert(entity, origin);
    }

    /// The entity that created `uuid`, when the region published one.
    pub fn origin_of(&self, uuid: Uuid) -> Option<Uuid> {
        self.origins.get(&uuid).copied()
    }

    /// Withdraws one entry (a provisional `execute summon` entry whose spawn was refused).
    pub fn forget(&mut self, uuid: Uuid) {
        self.entries.remove(&uuid);
        self.origins.remove(&uuid);
    }

    /// Adds or replaces one snapshot entry (used by tests and by `sync`).
    pub fn insert(&mut self, entity: CommandEntity) {
        self.entries.insert(entity.uuid, entity);
    }

    /// Replaces this region's entries with a fresh snapshot of `store`. `dimension` is stamped
    /// on every entry so leash resolution can reject holders outside the local dimension.
    ///
    /// `owned` is the set of UUIDs this index currently holds for the region; it is cleared
    /// and filled with the entities the store owns after the update, so a removed entity
    /// disappears from the command view on the next tick.
    pub fn sync(
        &mut self,
        owned: &mut HashSet<Uuid>,
        store: &EntityStore,
        dimension: super::Dimension,
    ) {
        for uuid in owned.iter() {
            self.entries.remove(uuid);
            self.origins.remove(uuid);
        }
        owned.clear();
        for key in store.keys() {
            let Some(data) = store.data(*key) else {
                continue;
            };
            if store
                .flags(*key)
                .is_some_and(|flags| flags.has(Flags::REMOVED))
            {
                continue;
            }
            let Some(pos) = store.pos(*key) else {
                continue;
            };
            let entity = CommandEntity {
                uuid: data.uuid,
                network_id: data.id,
                kind: data.kind,
                pos: [pos.x, pos.y, pos.z],
                yaw: data.yaw,
                pitch: data.pitch,
                dimension,
                tags: data.tags.clone(),
                custom_name: None,
                health: data.mob.as_ref().map(|mob| mob.health),
                attacker: data.mob.as_ref().and_then(|mob| mob.last_killer),
                owner: data.mob.as_ref().and_then(|mob| mob.owner),
                leasher: data
                    .mob
                    .as_ref()
                    .and_then(|mob| mob.leash.map(|(holder, _)| holder)),
                target: data.mob.as_ref().and_then(|mob| mob.target),
            };
            let origin = data
                .arrow
                .as_ref()
                .and_then(|arrow| arrow.owner)
                .or_else(|| data.item.as_ref().and_then(|item| item.thrower))
                .or_else(|| data.bobber.as_ref().map(|bobber| bobber.owner));
            if let Some(origin) = origin {
                self.set_origin(data.uuid, origin);
            }
            owned.insert(entity.uuid);
            self.insert(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::{EntityData, EntityRecord, allocate_id, random_uuid};
    use ferrite_math::Vec3;

    fn record(x: f64, y: f64, z: f64) -> EntityRecord {
        EntityRecord {
            pos: Vec3::new(x, y, z),
            vel: Vec3::ZERO,
            flags: Flags::default(),
            data: EntityData::new(allocate_id(), random_uuid(), 71, 0.25, 0.25),
        }
    }

    #[test]
    fn sync_publishes_and_forgets_region_entities() {
        let mut store = EntityStore::default();
        let key = store.insert(record(1.5, 64.0, 2.5));
        let uuid = store.data(key).unwrap().uuid;
        store.data_mut(key).unwrap().tags.insert("boss".into());

        let mut index = EntityIndex::default();
        let mut owned = HashSet::new();
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert_eq!(owned, HashSet::from([uuid]));
        let entry = index.get(uuid).expect("published entity");
        assert_eq!(entry.pos, [1.5, 64.0, 2.5]);
        assert_eq!(entry.dimension, crate::game::Dimension::Overworld);
        assert!(entry.tags.contains("boss"));

        store.remove(key);
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert_eq!(index.entries().count(), 0);
        assert!(owned.is_empty());

        // A removed entity is not published even before its slot is swept.
        let key = store.insert(record(3.5, 64.0, 3.5));
        let uuid = store.data(key).unwrap().uuid;
        store.flags_mut(key).unwrap().set(Flags::REMOVED, true);
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert!(index.get(uuid).is_none());
    }

    #[test]
    fn sync_publishes_a_mobs_relations_for_execute_on() {
        use ferrite_entity::mob::{MobKind, new_mob};
        let mut store = EntityStore::default();
        let owner = Uuid::from_u128(0x0a);
        let target = Uuid::from_u128(0x0b);
        let wolf = store.insert(new_mob(
            MobKind::Wolf,
            Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(3),
        ));
        let key = store.data(wolf).map(|data| data.uuid).unwrap();
        {
            let state = ferrite_entity::mob::state_mut(&mut store, wolf).unwrap();
            state.owner = Some(owner);
            state.leash = Some((owner, 7));
            state.target = Some(target);
            state.last_killer = Some(target);
        }
        let mut index = EntityIndex::default();
        index.sync(
            &mut HashSet::new(),
            &store,
            crate::game::Dimension::Overworld,
        );
        let entry = index.get(key).expect("published wolf");
        assert_eq!(entry.owner, Some(owner));
        assert_eq!(entry.leasher, Some(owner));
        assert_eq!(entry.target, Some(target));
        assert_eq!(entry.attacker, Some(target));
    }

    #[test]
    fn sync_publishes_a_mobs_current_health() {
        use ferrite_entity::mob::{MobKind, new_mob};
        let mut store = EntityStore::default();
        let pig = store.insert(new_mob(
            MobKind::Pig,
            Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(3),
        ));
        let plain = store.insert(record(5.5, 64.0, 5.5));
        ferrite_entity::mob::state_mut(&mut store, pig)
            .unwrap()
            .health = 4.0;
        let mut index = EntityIndex::default();
        index.sync(
            &mut HashSet::new(),
            &store,
            crate::game::Dimension::Overworld,
        );
        let uuid = store.data(pig).unwrap().uuid;
        assert_eq!(index.get(uuid).unwrap().health, Some(4.0));
        let other = store.data(plain).unwrap().uuid;
        assert_eq!(index.get(other).unwrap().health, None);
    }

    #[test]
    fn sync_publishes_who_created_an_arrow_or_threw_an_item() {
        use ferrite_entity::Rng;
        let shooter = Uuid::from_u128(0x5);
        let mut store = EntityStore::default();
        let arrow = store.insert(ferrite_entity::arrow::new_arrow(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::ZERO,
            2.0,
            Some(shooter),
            &mut Rng::new(1),
        ));
        let plain = store.insert(record(5.5, 64.0, 5.5));
        let mut index = EntityIndex::default();
        let mut owned = HashSet::new();
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        let arrow_uuid = store.data(arrow).unwrap().uuid;
        assert_eq!(index.origin_of(arrow_uuid), Some(shooter));
        assert_eq!(index.origin_of(store.data(plain).unwrap().uuid), None);
        store.remove(arrow);
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert_eq!(
            index.origin_of(arrow_uuid),
            None,
            "forgotten with the entity"
        );
    }
}
