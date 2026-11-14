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
    /// Item slots the owning region published for an entity (`/execute if items|slots entity`):
    /// the vanilla slot name and the item id of a stack of one. Only what `ferrite-entity`
    /// actually models is published (the horse armour in `armor.body`); an absent slot is empty
    /// or unmodelled, never guessed.
    slots: BTreeMap<Uuid, Vec<(&'static str, u16)>>,
    /// Player inventories read through each player's mailbox for the command being run
    /// (`publish_player_slots`): `(slot index as `slot_ops::player_slots` numbers it, item id)`
    /// for every non-empty slot. A player with no entry has not been read for this command.
    player_slots: BTreeMap<Uuid, Vec<(i16, u16)>>,
}

/// Every player slot index `slot_ops::player_slots` can name: the selected hotbar slot (`-1`),
/// the 36 container slots, four armour slots and the offhand.
const PLAYER_SLOTS: std::ops::RangeInclusive<i16> = -1..=40;

/// How long a command waits for the owning regions to answer inventory reads.
pub const PLAYER_SLOT_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether `command` can ask about inventories (`if|unless items|slots entity`), so the caller
/// only pays for the mailbox round trip when it is needed.
pub fn reads_entity_slots(command: &str) -> bool {
    (command.contains("items") || command.contains("slots")) && command.contains("entity")
}

/// Reads the inventory of every given player through their mailbox and publishes it into
/// `index`, replacing the previous reading. The owning region answers from its own state; nothing
/// here touches another region's live data. A player who does not answer within `wait` stays
/// unpublished, which the command reports instead of guessing.
pub fn publish_player_slots<'a>(
    index: &std::sync::Mutex<EntityIndex>,
    mailboxes: impl IntoIterator<Item = (Uuid, &'a super::region::Mailbox)>,
    wait: std::time::Duration,
) {
    let slots: Vec<i16> = PLAYER_SLOTS.collect();
    let mut pending = Vec::new();
    for (uuid, mailbox) in mailboxes {
        let (reply, received) = std::sync::mpsc::channel();
        mailbox.lock().unwrap_or_else(|e| e.into_inner()).push_back(
            super::region::Command::ReadSlots {
                slots: slots.clone(),
                reply,
            },
        );
        pending.push((uuid, received));
    }
    let deadline = std::time::Instant::now() + wait;
    let mut read = Vec::new();
    for (uuid, received) in pending {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(stacks) = received.recv_timeout(left) else {
            continue;
        };
        // The region drops slots it cannot resolve; a short answer cannot be aligned with the
        // request, so it is not published.
        if stacks.len() != slots.len() {
            continue;
        }
        let held = slots
            .iter()
            .zip(&stacks)
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| (*slot, stack.item))
            .collect();
        read.push((uuid, held));
    }
    let mut index = index.lock().unwrap_or_else(|e| e.into_inner());
    index.player_slots.clear();
    index.player_slots.extend(read);
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

    /// Records one published item slot of `entity`.
    pub fn set_slot(&mut self, entity: Uuid, slot: &'static str, item: u16) {
        self.slots.entry(entity).or_default().push((slot, item));
    }

    /// The item slots published for `uuid` as `(slot name, item id)`.
    pub fn slots_of(&self, uuid: Uuid) -> &[(&'static str, u16)] {
        self.slots.get(&uuid).map_or(&[], Vec::as_slice)
    }

    /// The non-empty slots read for player `uuid` as `(slot index, item id)`; `None` when the
    /// player was not read for the command being run.
    pub fn player_slots_of(&self, uuid: Uuid) -> Option<&[(i16, u16)]> {
        self.player_slots.get(&uuid).map(Vec::as_slice)
    }

    /// Records a player's slot reading (used by `publish_player_slots` and by tests).
    pub fn set_player_slots(&mut self, uuid: Uuid, held: Vec<(i16, u16)>) {
        self.player_slots.insert(uuid, held);
    }

    /// Drops every player reading, so a later command never sees an old inventory.
    pub fn clear_player_slots(&mut self) {
        self.player_slots.clear();
    }

    /// The entity that created `uuid`, when the region published one.
    pub fn origin_of(&self, uuid: Uuid) -> Option<Uuid> {
        self.origins.get(&uuid).copied()
    }

    /// Withdraws one entry (a provisional `execute summon` entry whose spawn was refused).
    pub fn forget(&mut self, uuid: Uuid) {
        self.entries.remove(&uuid);
        self.origins.remove(&uuid);
        self.slots.remove(&uuid);
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
            self.slots.remove(uuid);
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
            if let Some(mob) = data.mob.as_ref().filter(|mob| mob.armor != 0) {
                self.set_slot(data.uuid, "armor.body", mob.armor);
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
    fn player_slots_are_read_through_the_mailbox_and_replace_the_last_reading() {
        use super::super::region::{Command, Mailbox};
        use ferrite_item::ItemStack;
        let player = Uuid::from_u128(0x61);
        let silent = Uuid::from_u128(0x62);
        let mailbox = Mailbox::default();
        let quiet = Mailbox::default();
        let sword = ferrite_item::item_id("minecraft:diamond_sword").expect("item");
        let answering = {
            let mailbox = mailbox.clone();
            std::thread::spawn(move || {
                // A stand-in for the owning region: answers one `ReadSlots` from its inventory,
                // a sword in the selected slot (-1) and dirt in container slot 8.
                let dirt = ferrite_item::item_id("minecraft:dirt").expect("item");
                loop {
                    let command = mailbox.lock().unwrap().pop_front();
                    if let Some(Command::ReadSlots { slots, reply }) = command {
                        let stacks = slots
                            .iter()
                            .map(|slot| match slot {
                                -1 => ItemStack::with_count(sword, 1),
                                8 => ItemStack::with_count(dirt, 64),
                                _ => ItemStack::empty(),
                            })
                            .collect();
                        let _ = reply.send(stacks);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            })
        };
        let index = std::sync::Mutex::new(EntityIndex::default());
        index.lock().unwrap().set_player_slots(silent, vec![(0, 1)]);
        publish_player_slots(
            &index,
            [(player, &mailbox), (silent, &quiet)],
            std::time::Duration::from_millis(400),
        );
        answering.join().unwrap();
        let index = index.lock().unwrap();
        let dirt = ferrite_item::item_id("minecraft:dirt").expect("item");
        assert_eq!(
            index.player_slots_of(player),
            Some(&[(-1, sword), (8, dirt)][..])
        );
        // A player who did not answer is unpublished, and the earlier reading is gone.
        assert_eq!(index.player_slots_of(silent), None);
        // Nothing was queued for the quiet player beyond the one read it never answered.
        assert_eq!(quiet.lock().unwrap().len(), 1);
    }

    #[test]
    fn only_commands_that_can_ask_about_inventories_trigger_a_read() {
        assert!(reads_entity_slots(
            "execute if items entity @s weapon.mainhand *"
        ));
        assert!(reads_entity_slots(
            "execute unless slots entity @a armor.head run say x"
        ));
        assert!(!reads_entity_slots(
            "execute if items block 0 0 0 container.0 *"
        ));
        assert!(!reads_entity_slots("say hello"));
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

    #[test]
    fn sync_publishes_the_horse_armour_slot_and_forgets_it() {
        use ferrite_entity::mob::{MobKind, new_mob};
        let iron = ferrite_item::item_id("minecraft:iron_horse_armor").expect("item");
        let mut store = EntityStore::default();
        let horse = store.insert(new_mob(
            MobKind::Horse,
            Vec3::new(0.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(3),
        ));
        let bare = store.insert(new_mob(
            MobKind::Pig,
            Vec3::new(4.5, 64.0, 0.5),
            &mut ferrite_entity::Rng::new(3),
        ));
        ferrite_entity::mob::state_mut(&mut store, horse)
            .unwrap()
            .armor = iron;
        let (horse_id, bare_id) = (
            store.data(horse).unwrap().uuid,
            store.data(bare).unwrap().uuid,
        );
        let mut index = EntityIndex::default();
        let mut owned = HashSet::new();
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert_eq!(index.slots_of(horse_id), &[("armor.body", iron)]);
        assert!(index.slots_of(bare_id).is_empty());
        ferrite_entity::mob::state_mut(&mut store, horse)
            .unwrap()
            .armor = 0;
        index.sync(&mut owned, &store, crate::game::Dimension::Overworld);
        assert!(
            index.slots_of(horse_id).is_empty(),
            "removed armour is forgotten"
        );
    }
}
