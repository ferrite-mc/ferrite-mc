//! Per-chunk entity persistence.
//!
//! Vanilla writes a chunk's entities into that chunk's `Entities` list. Ferrite keeps entities
//! in region-local [`ferrite_entity::EntityStore`]s rather than in
//! [`ferrite_world::Chunk`], so this module stores them in a sidecar next to the region files:
//!
//! ```text
//! <world>/entities/c.<x>.<z>.nbt   (gzip-compressed named NBT)
//! ```
//!
//! The root compound carries `DataVersion` and an `Entities` list in the vanilla save shape
//! (`id`, `UUID`, `Pos`, `Motion`, `Rotation`, `TickCount`, plus the per-type fields this server
//! models). A chunk's file is written when its last holder releases it and when the server shuts
//! down, and read back the first time the chunk is held again. Players are not saved here;
//! `playerdata` owns them.
//!
//! Unknown entity ids and malformed records are skipped with a warning; the rest of the chunk
//! still loads. Entity UUIDs are stable across a save/reload, so item stacks and `/tag` names
//! keep their owners. Effects and metadata beyond the item stack are not persisted yet.

use ferrite_entity::{
    EntityData, EntityRecord, Flags, allocate_id, arrow::ArrowState, experience::OrbState,
    item::ItemState, mob::MobKind, mob::MobState, random_uuid,
};
use ferrite_math::Vec3;
use ferrite_nbt::{Compound, List, Tag, binary};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Directory below the world folder that holds the per-chunk entity sidecars.
pub const DIRECTORY: &str = "entities";
/// Chunk data version written into each sidecar (`level.dat`/chunk save format of 26.3).
const DATA_VERSION: i32 = 5023;

/// Sidecar path for a chunk.
pub fn chunk_file(world: &Path, (x, z): (i32, i32)) -> PathBuf {
    world.join(DIRECTORY).join(format!("c.{x}.{z}.nbt"))
}

/// Writes one chunk's entities, or removes the file when there are none left.
pub fn save(world: &Path, chunk: (i32, i32), records: &[EntityRecord]) -> std::io::Result<()> {
    let path = chunk_file(world, chunk);
    if records.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }
    let bytes = binary::write_compressed("entities", &to_nbt(records), binary::Compression::Gzip)
        .map_err(std::io::Error::other)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("nbt.tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, path)
}

/// Reads one chunk's entities. A missing, unreadable or corrupt file yields no entities (and a
/// warning) instead of failing the chunk load.
pub fn load(world: &Path, chunk: (i32, i32)) -> Vec<EntityRecord> {
    let path = chunk_file(world, chunk);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring an unreadable entity file");
            return Vec::new();
        }
    };
    match binary::read_compressed(&bytes, binary::Compression::Gzip) {
        Ok((_, tag)) => from_nbt(&tag),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring a corrupt entity file");
            Vec::new()
        }
    }
}

/// Writes every entity of every region, grouped per chunk, for shutdown. Regions are consumed by
/// the scheduler shutdown, so this is called after the workers have stopped.
pub fn save_regions(world: &Path, regions: &mut [super::region::Region]) -> std::io::Result<()> {
    let mut chunks: std::collections::BTreeMap<(i32, i32), Vec<EntityRecord>> =
        std::collections::BTreeMap::new();
    for region in regions {
        for record in region.drain_entities() {
            chunks.entry(record.chunk()).or_default().push(record);
        }
    }
    for (chunk, records) in chunks {
        save(world, chunk, &records)?;
    }
    Ok(())
}

/// Serializes a chunk's entities into the sidecar root compound.
pub fn to_nbt(records: &[EntityRecord]) -> Tag {
    let mut root = Compound::new();
    root.insert("DataVersion".into(), Tag::Int(DATA_VERSION));
    let entities = records
        .iter()
        .filter_map(|record| {
            entity_to_nbt(record).or_else(|| {
                tracing::warn!(
                    uuid = %record.data.uuid,
                    "skipping an entity that cannot be saved"
                );
                None
            })
        })
        .collect();
    root.insert("Entities".into(), Tag::List(List::Compound(entities)));
    Tag::Compound(root)
}

/// Reads a sidecar root compound back into entity records.
pub fn from_nbt(tag: &Tag) -> Vec<EntityRecord> {
    let Tag::Compound(root) = tag else {
        tracing::warn!("ignoring an entity file whose root is not a compound");
        return Vec::new();
    };
    let Some(Tag::List(List::Compound(entities))) = root.get("Entities") else {
        tracing::warn!("ignoring an entity file without an Entities list");
        return Vec::new();
    };
    entities.iter().filter_map(entity_from_nbt).collect()
}

fn uuid_words(uuid: Uuid) -> Vec<i32> {
    let n = uuid.as_u128();
    vec![
        (n >> 96) as u32 as i32,
        (n >> 64) as u32 as i32,
        (n >> 32) as u32 as i32,
        n as u32 as i32,
    ]
}

fn uuid_from_words(words: &[i32]) -> Option<Uuid> {
    if words.len() != 4 {
        return None;
    }
    let mut n: u128 = 0;
    for word in words {
        n = (n << 32) | u128::from(*word as u32);
    }
    Some(Uuid::from_u128(n))
}

fn vec3_tag(value: Vec3) -> Tag {
    Tag::List(List::Double(vec![value.x, value.y, value.z]))
}

fn vec3(c: &Compound, key: &str) -> Option<Vec3> {
    match c.get(key) {
        Some(Tag::List(List::Double(v))) if v.len() == 3 => Some(Vec3::new(v[0], v[1], v[2])),
        _ => None,
    }
}

fn int(c: &Compound, key: &str) -> Option<i32> {
    match c.get(key) {
        Some(Tag::Int(value)) => Some(*value),
        Some(Tag::Short(value)) => Some(i32::from(*value)),
        _ => None,
    }
}

fn float(c: &Compound, key: &str) -> Option<f32> {
    match c.get(key) {
        Some(Tag::Float(value)) => Some(*value),
        Some(Tag::Double(value)) => Some(*value as f32),
        _ => None,
    }
}

fn boolean(c: &Compound, key: &str) -> bool {
    matches!(c.get(key), Some(Tag::Byte(value)) if *value != 0)
}

fn uuid_field(c: &Compound, key: &str) -> Option<Uuid> {
    match c.get(key) {
        Some(Tag::IntArray(words)) => uuid_from_words(words),
        _ => None,
    }
}

/// Writes a villager stack as an item compound; `None` for an empty or unencodable stack.
fn item_stack_compound(stack: &ferrite_item::ItemStack) -> Option<Compound> {
    (!stack.is_empty()).then(|| stack.to_nbt().ok()).flatten()
}

/// One merchant offer of a saved villager.
fn offer_to_nbt(offer: &ferrite_inventory::Trade) -> Option<Compound> {
    let mut c = Compound::new();
    c.insert(
        "wants".into(),
        Tag::Compound(item_stack_compound(&offer.wants)?),
    );
    c.insert(
        "gives".into(),
        Tag::Compound(item_stack_compound(&offer.gives)?),
    );
    if let Some(second) = &offer.second_wants {
        c.insert(
            "second_wants".into(),
            Tag::Compound(item_stack_compound(second)?),
        );
    }
    c.insert("uses".into(), Tag::Int(offer.uses));
    c.insert("max_uses".into(), Tag::Int(offer.max_uses));
    c.insert("xp".into(), Tag::Int(offer.xp));
    c.insert("special_price".into(), Tag::Int(offer.special_price));
    c.insert(
        "price_multiplier".into(),
        Tag::Float(offer.price_multiplier),
    );
    c.insert("demand".into(), Tag::Int(offer.demand));
    Some(c)
}

fn offer_from_nbt(c: &Compound) -> Option<ferrite_inventory::Trade> {
    let stack = |key: &str| match c.get(key) {
        Some(Tag::Compound(tag)) => ferrite_item::ItemStack::from_nbt(tag).ok(),
        _ => None,
    };
    let wants = stack("wants")?;
    let gives = stack("gives")?;
    if wants.is_empty() || gives.is_empty() {
        return None;
    }
    let max_uses = int(c, "max_uses").unwrap_or(12).max(1);
    let mut trade =
        ferrite_inventory::Trade::new(wants, gives, max_uses, int(c, "xp").unwrap_or(0));
    trade.second_wants = stack("second_wants");
    trade.uses = int(c, "uses").unwrap_or(0).clamp(0, max_uses);
    trade.special_price = int(c, "special_price").unwrap_or(0);
    trade.price_multiplier = float(c, "price_multiplier").unwrap_or(0.05);
    trade.demand = int(c, "demand").unwrap_or(0).max(0);
    Some(trade)
}

/// Writes the villager-specific fields of an entity compound: profession, offers and the
/// gossip/reputation this villager holds about players.
fn villager_to_nbt(state: &ferrite_entity::villager::VillagerState, c: &mut Compound) {
    let mut data = Compound::new();
    data.insert(
        "type".into(),
        Tag::String(state.villager_type.name().to_owned()),
    );
    data.insert(
        "profession".into(),
        Tag::String(state.profession.name().to_owned()),
    );
    data.insert("level".into(), Tag::Int(i32::from(state.level)));
    c.insert("VillagerData".into(), Tag::Compound(data));
    c.insert("Xp".into(), Tag::Int(state.xp));
    if let Some([x, y, z]) = state.workstation {
        c.insert("Workstation".into(), Tag::IntArray(vec![x, y, z]));
    }
    c.insert("RestockTimer".into(), Tag::Int(state.restock_timer as i32));
    c.insert(
        "SearchTimer".into(),
        Tag::Int(i32::from(state.search_timer)),
    );
    c.insert("Home".into(), Tag::List(List::Double(state.home.to_vec())));
    c.insert(
        "Offers".into(),
        Tag::List(List::Compound(
            state.offers.iter().filter_map(offer_to_nbt).collect(),
        )),
    );
    let gossips: Vec<Compound> = state
        .gossips
        .entries()
        .map(|(player, kind, value)| {
            let mut entry = Compound::new();
            entry.insert("Target".into(), Tag::IntArray(uuid_words(player)));
            entry.insert("Type".into(), Tag::String(kind.name().to_owned()));
            entry.insert("Value".into(), Tag::Int(value));
            entry
        })
        .collect();
    c.insert("Gossips".into(), Tag::List(List::Compound(gossips)));
}

fn gossips_from_nbt(c: &Compound) -> ferrite_entity::villager::Gossips {
    use ferrite_entity::villager::{GossipType, Gossips};
    let mut gossips = Gossips::default();
    let Some(Tag::List(List::Compound(entries))) = c.get("Gossips") else {
        return gossips;
    };
    for entry in entries {
        let (Some(target), Some(Tag::String(kind)), Some(value)) = (
            uuid_field(entry, "Target"),
            entry.get("Type"),
            int(entry, "Value"),
        ) else {
            continue;
        };
        if let Some(kind) = GossipType::from_name(kind) {
            gossips.set(target, kind, value);
        }
    }
    gossips
}

/// Reads a villager's trade state; `None` when the record has no `VillagerData`.
fn villager_from_nbt(
    c: &Compound,
    uuid: Uuid,
    pos: Vec3,
) -> Option<ferrite_entity::villager::VillagerState> {
    use ferrite_entity::villager::{self, Profession, VillagerState, VillagerType};
    let Tag::Compound(data) = c.get("VillagerData")? else {
        return None;
    };
    let name = |key: &str| match data.get(key) {
        Some(Tag::String(name)) => Some(name.as_str()),
        _ => None,
    };
    let mut state = VillagerState::new(
        name("type")
            .and_then(VillagerType::from_name)
            .unwrap_or_default(),
        [pos.x, pos.y, pos.z],
    );
    state.profession = name("profession")
        .and_then(Profession::from_name)
        .unwrap_or_default();
    state.level = int(data, "level")
        .unwrap_or(1)
        .clamp(1, i32::from(villager::MAX_LEVEL)) as u8;
    state.xp = int(c, "Xp").unwrap_or(0);
    state.workstation = match c.get("Workstation") {
        Some(Tag::IntArray(words)) if words.len() == 3 => Some([words[0], words[1], words[2]]),
        _ => None,
    };
    state.restock_timer = int(c, "RestockTimer")
        .unwrap_or(villager::RESTOCK_TICKS as i32)
        .max(0) as u32;
    state.search_timer = int(c, "SearchTimer")
        .unwrap_or(0)
        .clamp(0, i32::from(u16::MAX)) as u16;
    if let Some(home) = vec3(c, "Home") {
        state.home = [home.x, home.y, home.z];
    }
    if let Some(Tag::List(List::Compound(offers))) = c.get("Offers") {
        state.offers = offers.iter().filter_map(offer_from_nbt).collect();
    }
    state.gossips = gossips_from_nbt(c);
    // A record saved before offers were persisted gets the same deterministic draw back.
    if state.offers.is_empty() && !matches!(state.profession, Profession::None | Profession::Nitwit)
    {
        state.offers = villager::offers_for(state.profession, state.level, uuid);
    }
    Some(state)
}

fn entity_to_nbt(record: &EntityRecord) -> Option<Compound> {
    let data = &record.data;
    let props = ferrite_data::entity_props::get(data.kind);
    let mut c = Compound::new();
    c.insert("id".into(), Tag::String(props.key.to_owned()));
    c.insert("UUID".into(), Tag::IntArray(uuid_words(data.uuid)));
    c.insert("Pos".into(), vec3_tag(record.pos));
    c.insert("Motion".into(), vec3_tag(record.vel));
    c.insert(
        "Rotation".into(),
        Tag::List(List::Float(vec![data.yaw, data.pitch])),
    );
    c.insert("TickCount".into(), Tag::Int(data.tick_count as i32));
    c.insert(
        "OnGround".into(),
        Tag::Byte(record.flags.has(Flags::ON_GROUND) as i8),
    );
    c.insert(
        "NoGravity".into(),
        Tag::Byte(record.flags.has(Flags::NO_GRAVITY) as i8),
    );
    c.insert(
        "NoPhysics".into(),
        Tag::Byte(record.flags.has(Flags::NO_PHYSICS) as i8),
    );
    if !data.tags.is_empty() {
        c.insert(
            "Tags".into(),
            Tag::List(List::String(data.tags.iter().cloned().collect())),
        );
    }

    if let Some(item) = &data.item {
        // The entity's synched slot carries only an item id and a count (the protocol stack has
        // no component patch), so that is what the sidecar stores.
        let stack = ferrite_entity::item::stack(data);
        let stack = u16::try_from(stack.item_id)
            .ok()
            .map(|id| ferrite_item::ItemStack::with_count(id, stack.count));
        if let Some(stack) = stack.filter(|stack| !stack.is_empty()) {
            c.insert("Item".into(), Tag::Compound(stack.to_nbt().ok()?));
        }
        c.insert("Age".into(), Tag::Int(item.age));
        c.insert("PickupDelay".into(), Tag::Int(item.pickup_delay));
        c.insert("ItemHealth".into(), Tag::Int(item.health));
        if let Some(target) = item.target {
            c.insert("PickupTarget".into(), Tag::IntArray(uuid_words(target)));
        }
    }

    if let Some(arrow) = &data.arrow {
        c.insert("damage".into(), Tag::Double(f64::from(arrow.damage)));
        c.insert("crit".into(), Tag::Byte(arrow.critical as i8));
        c.insert("life".into(), Tag::Int(arrow.age as i32));
        if let Some(owner) = arrow.owner {
            c.insert("owner".into(), Tag::IntArray(uuid_words(owner)));
        }
    }

    if let Some(mob) = &data.mob {
        c.insert("MobKind".into(), Tag::String(mob.kind.name().to_owned()));
        c.insert("Health".into(), Tag::Float(mob.health));
        c.insert("HurtTime".into(), Tag::Short(mob.hurt_time as i16));
        c.insert("NoActionTime".into(), Tag::Int(mob.no_action_time as i32));
        c.insert("FallDistance".into(), Tag::Float(mob.fall_distance));
        c.insert("Despawns".into(), Tag::Byte(mob.despawns as i8));
        c.insert("LootDropped".into(), Tag::Byte(mob.loot_dropped as i8));
        c.insert("NoAI".into(), Tag::Byte(mob.no_ai as i8));
        c.insert("WanderTimer".into(), Tag::Int(mob.wander_timer as i32));
        c.insert("Age".into(), Tag::Int(mob.age));
        if let Some(owner) = mob.owner {
            c.insert("Owner".into(), Tag::IntArray(uuid_words(owner)));
        }
        c.insert("Sitting".into(), Tag::Byte(mob.sitting as i8));
        c.insert("Saddled".into(), Tag::Byte(mob.saddled as i8));
        c.insert("Armor".into(), Tag::Int(i32::from(mob.armor)));
        if let Some((holder, _)) = mob.leash {
            c.insert("Leash".into(), Tag::IntArray(uuid_words(holder)));
        }
        if mob.chest {
            c.insert("ChestedHorse".into(), Tag::Byte(1));
            c.insert("ChestItems".into(), super::equine_chest::to_nbt(data.uuid));
        }
        c.insert("ArmorPoints".into(), Tag::Float(mob.armor_points));
        c.insert("HorseSpeed".into(), Tag::Float(mob.speed));
        c.insert("HorseJump".into(), Tag::Float(mob.jump));
        c.insert("Temper".into(), Tag::Int(mob.temper));
        if mob.kind.is_llama() {
            c.insert(
                "Strength".into(),
                Tag::Int(ferrite_entity::horse::strength(data)),
            );
        }
        c.insert("InLove".into(), Tag::Int(i32::from(mob.love)));
        c.insert("WanderYaw".into(), Tag::Float(mob.wander_yaw));
        c.insert("Moving".into(), Tag::Byte(mob.moving as i8));
        c.insert(
            "AttackCooldown".into(),
            Tag::Short(mob.attack_cooldown as i16),
        );
        c.insert("Panic".into(), Tag::Short(mob.panic as i16));
        c.insert("Swell".into(), Tag::Short(mob.swell as i16));
        c.insert("BowTimer".into(), Tag::Short(mob.bow_timer as i16));
        if let Some(target) = mob.target {
            c.insert("Target".into(), Tag::IntArray(uuid_words(target)));
        }
    }

    if let Some(villager) = &data.villager {
        villager_to_nbt(villager, &mut c);
    }

    if let Some(orb) = &data.orb {
        c.insert("OrbAge".into(), Tag::Int(orb.age));
        c.insert("OrbCount".into(), Tag::Int(orb.count));
        c.insert(
            "OrbValue".into(),
            Tag::Int(ferrite_entity::experience::value(data)),
        );
    }

    if let Some(fuse) = data.fuse {
        c.insert("fuse".into(), Tag::Int(fuse));
    }
    if let Some(state) = data.block_state {
        c.insert("BlockState".into(), Tag::Int(i32::from(state)));
    }
    Some(c)
}

fn entity_from_nbt(c: &Compound) -> Option<EntityRecord> {
    let Some(Tag::String(name)) = c.get("id") else {
        tracing::warn!("skipping a saved entity without an id");
        return None;
    };
    let Some(props) = ferrite_data::entity_props::by_name(name) else {
        tracing::warn!(id = %name, "dropping a saved entity of an unknown type");
        return None;
    };
    let Some(pos) = vec3(c, "Pos") else {
        tracing::warn!(id = %name, "skipping a saved entity without a valid Pos");
        return None;
    };
    let uuid = uuid_field(c, "UUID").unwrap_or_else(random_uuid);
    let mut data = EntityData::new(
        allocate_id(),
        uuid,
        props.id,
        f64::from(props.width),
        f64::from(props.height),
    );
    let vel = vec3(c, "Motion").unwrap_or(Vec3::ZERO);
    let mut flags = Flags::default();
    flags.set(Flags::ON_GROUND, boolean(c, "OnGround"));
    flags.set(Flags::NO_GRAVITY, boolean(c, "NoGravity"));
    flags.set(Flags::NO_PHYSICS, boolean(c, "NoPhysics"));
    if let Some(Tag::List(List::Float(rotation))) = c.get("Rotation")
        && rotation.len() == 2
    {
        data.yaw = rotation[0];
        data.pitch = rotation[1];
    }
    data.tick_count = int(c, "TickCount").unwrap_or(0).max(0) as u32;
    if let Some(Tag::List(List::String(tags))) = c.get("Tags") {
        data.tags = tags.iter().cloned().collect();
    }

    if let Some(Tag::Compound(item)) = c.get("Item")
        && let Ok(stack) = ferrite_item::ItemStack::from_nbt(item)
        && !stack.is_empty()
    {
        data.metadata.define(
            ferrite_entity::metadata::item::ITEM,
            ferrite_protocol::packets::play::MetaValue::ItemStack(Default::default()),
        );
        ferrite_entity::item::set_stack(
            &mut data,
            ferrite_protocol::packets::play::ItemStack {
                count: stack.count,
                item_id: i32::from(stack.item),
            },
        );
        data.item = Some(ItemState {
            age: int(c, "Age").unwrap_or(0),
            pickup_delay: int(c, "PickupDelay").unwrap_or(0),
            health: int(c, "ItemHealth").unwrap_or(5),
            target: uuid_field(c, "PickupTarget"),
        });
    }

    if c.contains_key("damage") {
        data.arrow = Some(ArrowState {
            damage: float(c, "damage").unwrap_or(0.0),
            owner: uuid_field(c, "owner"),
            critical: boolean(c, "crit"),
            age: int(c, "life").unwrap_or(0).max(0) as u32,
            channeling: boolean(c, "channeling"),
        });
    }

    if let Some(Tag::String(kind)) = c.get("MobKind") {
        match MobKind::from_name(kind) {
            Some(kind) => {
                let mut mob = MobState::new(kind);
                mob.health = float(c, "Health").unwrap_or(mob.health);
                mob.hurt_time = int(c, "HurtTime").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
                mob.no_action_time = int(c, "NoActionTime").unwrap_or(0).max(0) as u32;
                mob.fall_distance = float(c, "FallDistance").unwrap_or(0.0);
                mob.despawns = boolean(c, "Despawns");
                mob.loot_dropped = boolean(c, "LootDropped");
                mob.no_ai = boolean(c, "NoAI");
                mob.wander_timer = int(c, "WanderTimer").unwrap_or(0).max(0) as u32;
                mob.wander_yaw = float(c, "WanderYaw").unwrap_or(0.0);
                mob.moving = boolean(c, "Moving");
                mob.attack_cooldown = int(c, "AttackCooldown")
                    .unwrap_or(0)
                    .clamp(0, i16::MAX as i32) as u16;
                mob.panic = int(c, "Panic").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
                mob.swell = int(c, "Swell").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
                mob.bow_timer = int(c, "BowTimer").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
                mob.target = uuid_field(c, "Target");
                mob.age = int(c, "Age").unwrap_or(0);
                mob.love = int(c, "InLove").unwrap_or(0).clamp(0, i32::from(u16::MAX)) as u16;
                mob.owner = uuid_field(c, "Owner");
                mob.sitting = boolean(c, "Sitting");
                mob.saddled = boolean(c, "Saddled");
                // The holder's entity id is filled in by the region once the player is there.
                mob.leash = uuid_field(c, "Leash").map(|holder| (holder, 0));
                mob.chest = boolean(c, "ChestedHorse");
                if mob.chest
                    && let Some(items) = c.get("ChestItems")
                {
                    super::equine_chest::from_nbt(data.uuid, items);
                }
                mob.armor_points = float(c, "ArmorPoints").unwrap_or(0.0);
                mob.speed = float(c, "HorseSpeed").unwrap_or(0.0);
                mob.jump = float(c, "HorseJump").unwrap_or(0.0);
                mob.armor = int(c, "Armor").unwrap_or(0).clamp(0, i32::from(u16::MAX)) as u16;
                mob.temper = int(c, "Temper").unwrap_or(0);
                if kind.is_equine() {
                    ferrite_entity::horse::define_metadata(&mut data.metadata, &mob);
                }
                if kind.is_llama() {
                    ferrite_entity::horse::set_strength(&mut data, int(c, "Strength").unwrap_or(0));
                }
                if kind.is_animal() {
                    ferrite_entity::breed::define_metadata(&mut data.metadata, &mob);
                }
                if kind.is_tameable() {
                    ferrite_entity::tame::define_metadata(&mut data.metadata, &mob);
                }
                data.mob = Some(mob);
            }
            None => tracing::warn!(kind, "ignoring an unknown mob kind"),
        }
    }

    if let Some(kind @ (MobKind::Villager | MobKind::WanderingTrader)) =
        data.mob.as_ref().map(|mob| mob.kind)
    {
        let state = villager_from_nbt(c, data.uuid, pos).unwrap_or_else(|| {
            let mut state = ferrite_entity::villager::VillagerState::new(
                ferrite_entity::villager::VillagerType::Plains,
                [pos.x, pos.y, pos.z],
            );
            if kind == MobKind::WanderingTrader {
                state.offers = ferrite_entity::villager::trader_offers(data.uuid);
            }
            state
        });
        ferrite_entity::villager::define_metadata(&mut data.metadata, &state);
        data.villager = Some(state);
    }

    if c.contains_key("OrbCount") {
        // `new_orb` sets up the synched value accessor; the saved state overrides the rest.
        let mut record = ferrite_entity::experience::new_orb(pos, int(c, "OrbValue").unwrap_or(0));
        record.vel = vel;
        record.flags = flags;
        record.data.tick_count = data.tick_count;
        record.data.tags = data.tags;
        record.data.orb = Some(OrbState {
            age: int(c, "OrbAge").unwrap_or(0),
            count: int(c, "OrbCount").unwrap_or(1).max(1),
        });
        return Some(record);
    }

    data.fuse = int(c, "fuse");
    data.block_state = int(c, "BlockState").and_then(|state| u16::try_from(state).ok());
    Some(EntityRecord {
        pos,
        vel,
        flags,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::Rng;
    use ferrite_entity::arrow;
    use ferrite_entity::item::new_item;
    use ferrite_entity::mob;
    use ferrite_entity::tnt;

    fn temp_world(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrite-entities-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn mob_record() -> EntityRecord {
        let mut rng = Rng::default();
        let mut record = mob::new_mob(MobKind::Zombie, Vec3::new(12.5, 64.0, -3.5), &mut rng);
        record.data.yaw = 45.0;
        record.data.pitch = 10.0;
        record.data.mob.as_mut().unwrap().health = 12.5;
        record.data.mob.as_mut().unwrap().target = Some(Uuid::from_u128(99));
        record
    }

    fn item_record_item_id() -> i32 {
        i32::from(
            ferrite_item::ItemStack::from_name("diamond_sword")
                .unwrap()
                .item,
        )
    }

    fn item_record() -> EntityRecord {
        let mut rng = Rng::default();
        let stack = ferrite_item::ItemStack::from_name("diamond_sword")
            .unwrap()
            .copy_with_count(1);
        let stack = ferrite_protocol::packets::play::ItemStack {
            count: stack.count,
            item_id: i32::from(stack.item),
        };
        let mut record = new_item(
            Vec3::new(1.5, 65.0, 2.5),
            Vec3::new(0.0, 0.1, 0.0),
            stack,
            &mut rng,
        );
        record.data.item.as_mut().unwrap().pickup_delay = 10;
        record.data.item.as_mut().unwrap().age = 42;
        record
    }

    #[test]
    fn nbt_round_trip_keeps_mob_item_arrow_and_tnt_state() {
        let mut rng = Rng::default();
        let mob = mob_record();
        let item = item_record();
        let arrow = arrow::new_arrow(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            6.0,
            Some(Uuid::from_u128(7)),
            &mut rng,
        );
        let tnt = tnt::new_primed_tnt(Vec3::new(5.5, 64.0, 5.5), 80);
        let records = vec![mob.clone(), item, arrow, tnt];

        let tag = to_nbt(&records);
        let loaded = from_nbt(&tag);
        assert_eq!(loaded.len(), records.len());

        let loaded_mob = &loaded[0];
        let source_mob = &mob.data;
        assert_eq!(loaded_mob.data.uuid, source_mob.uuid);
        assert_eq!(loaded_mob.data.kind, source_mob.kind);
        assert_eq!(loaded_mob.data.mob.as_ref().unwrap().health, 12.5);
        assert_eq!(
            loaded_mob.data.mob.as_ref().unwrap().target,
            Some(Uuid::from_u128(99))
        );
        assert_eq!(loaded_mob.data.yaw, 45.0);

        let loaded_item = &loaded[1];
        assert_eq!(loaded_item.data.item.as_ref().unwrap().age, 42);
        assert_eq!(
            ferrite_entity::item::stack(&loaded_item.data).item_id,
            item_record_item_id()
        );
        assert_eq!(loaded_item.data.item.as_ref().unwrap().pickup_delay, 10);

        assert_eq!(loaded[2].data.arrow.as_ref().unwrap().damage, 6.0);
        assert_eq!(
            loaded[2].data.arrow.as_ref().unwrap().owner,
            Some(Uuid::from_u128(7))
        );
        assert_eq!(loaded[3].data.fuse, Some(80));
    }

    #[test]
    fn a_lead_and_a_saddle_survive_a_save() {
        let mut record = ferrite_entity::mob::new_mob(
            ferrite_entity::mob::MobKind::Llama,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let holder = Uuid::from_u128(5);
        let state = record.data.mob.as_mut().unwrap();
        state.leash = Some((holder, 12));
        state.armor = 4;
        let loaded = from_nbt(&to_nbt(&[record]));
        let state = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!(
            state.leash,
            Some((holder, 0)),
            "the entity id is set again on load"
        );
        assert_eq!(state.armor, 4);
    }

    #[test]
    fn horse_attributes_and_llama_strength_survive_a_save() {
        let mut rng = Rng::default();
        let mut horse = mob::new_mob(MobKind::Horse, Vec3::new(0.5, 64.0, 0.5), &mut rng);
        let horse_state = horse.data.mob.as_mut().unwrap();
        horse_state.speed = 0.2;
        horse_state.jump = 0.7;
        let mut llama = mob::new_mob(MobKind::Llama, Vec3::new(1.5, 64.0, 0.5), &mut rng);
        llama.data.mob.as_mut().unwrap().chest = true;
        ferrite_entity::horse::set_strength(&mut llama.data, 4);

        let loaded = from_nbt(&to_nbt(&[horse, llama]));
        let loaded_horse = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!((loaded_horse.speed, loaded_horse.jump), (0.2, 0.7));
        assert_eq!(ferrite_entity::horse::strength(&loaded[1].data), 4);
        assert_eq!(
            loaded[1]
                .data
                .metadata
                .get(ferrite_entity::horse::STRENGTH_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::Int(4))
        );
    }

    #[test]
    fn a_villagers_trade_state_and_gossip_survive_a_save() {
        let mut record = ferrite_entity::mob::new_mob(
            MobKind::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let uuid = record.data.uuid;
        let alice = Uuid::from_u128(0xa11ce);
        let bob = Uuid::from_u128(0xb0b);
        {
            let state = record.data.villager.as_mut().unwrap();
            state.profession = ferrite_entity::villager::Profession::Farmer;
            state.level = 2;
            state.xp = 30;
            state.workstation = Some([2, 64, 3]);
            state.restock_timer = 1234;
            state.offers =
                ferrite_entity::villager::offers_for(state.profession, state.level, uuid);
            state.offers[0].uses = 3;
            state.offers[0].demand = 2;
            state
                .gossips
                .add(alice, ferrite_entity::villager::GossipType::Trading, 20);
            state.gossips.add(
                alice,
                ferrite_entity::villager::GossipType::MajorPositive,
                20,
            );
            state
                .gossips
                .add(bob, ferrite_entity::villager::GossipType::MinorNegative, 25);
        }

        let loaded = from_nbt(&to_nbt(&[record]));
        assert_eq!(loaded.len(), 1);
        let state = loaded[0].data.villager.as_ref().expect("villager state");
        assert_eq!(
            state.profession,
            ferrite_entity::villager::Profession::Farmer
        );
        assert_eq!(state.level, 2);
        assert_eq!(state.xp, 30);
        assert_eq!(state.workstation, Some([2, 64, 3]));
        assert_eq!(state.restock_timer, 1234);
        assert_eq!(state.offers.len(), 4, "level 1 and 2 offers are restored");
        assert_eq!(state.offers[0].uses, 3);
        assert_eq!(state.offers[0].demand, 2);
        assert_eq!(state.reputation(alice), 120);
        assert_eq!(state.reputation(bob), -25);
        assert_eq!(
            loaded[0]
                .data
                .metadata
                .get(ferrite_entity::villager::VILLAGER_DATA_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::VillagerData {
                kind: ferrite_entity::villager::VillagerType::Plains.id(),
                profession: ferrite_entity::villager::Profession::Farmer.id(),
                level: 2,
            })
        );
    }

    #[test]
    fn unknown_entity_ids_are_dropped_and_the_rest_loads() {
        let good = mob_record();
        let tag = to_nbt(&[good]);
        let mut root = match tag {
            Tag::Compound(root) => root,
            _ => unreachable!(),
        };
        let Tag::List(List::Compound(mut list)) = root.shift_remove("Entities").unwrap() else {
            unreachable!()
        };
        let mut bad = Compound::new();
        bad.insert(
            "id".into(),
            Tag::String("minecraft:not_a_real_entity".into()),
        );
        bad.insert("Pos".into(), vec3_tag(Vec3::new(0.0, 0.0, 0.0)));
        list.push(bad);
        root.insert("Entities".into(), Tag::List(List::Compound(list)));
        let loaded = from_nbt(&Tag::Compound(root));
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn files_round_trip_and_empty_saves_remove_the_chunk_file() {
        let world = temp_world("files");
        let records = vec![mob_record(), item_record()];
        save(&world, (2, -3), &records).unwrap();
        assert!(chunk_file(&world, (2, -3)).exists());
        let loaded = load(&world, (2, -3));
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].data.uuid, records[0].data.uuid);
        save(&world, (2, -3), &[]).unwrap();
        assert!(!chunk_file(&world, (2, -3)).exists());
        assert!(load(&world, (2, -3)).is_empty());
        let _ = std::fs::remove_dir_all(world);
    }

    #[test]
    fn corrupt_files_load_as_empty() {
        let world = temp_world("corrupt");
        let path = chunk_file(&world, (0, 0));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not nbt").unwrap();
        assert!(load(&world, (0, 0)).is_empty());
        let _ = std::fs::remove_dir_all(world);
    }
}
