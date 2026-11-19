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
    EntityData, EntityRecord, Flags, allocate_id, arrow::ArrowState, dragon::DragonPhase,
    experience::OrbState, item::ItemState, mob::MobKind, mob::MobState, random_uuid,
    shoulder::Shoulders,
};
use ferrite_math::Vec3;
use ferrite_nbt::{Compound, List, Tag, binary};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Directory below the world folder that holds the per-chunk entity sidecars.
pub const DIRECTORY: &str = "entities";
/// Key of the vehicle a saved rider sits on (a UUID as four ints).
const RIDE_VEHICLE: &str = "RideVehicle";
/// Chunk data version written into each sidecar (`level.dat`/chunk save format of 26.3).
const DATA_VERSION: i32 = 5023;

/// Sidecar path for a chunk.
pub fn chunk_file(world: &Path, (x, z): (i32, i32)) -> PathBuf {
    world.join(DIRECTORY).join(format!("c.{x}.{z}.nbt"))
}

/// Sidecar path of a player's shoulder parrots, beside that player's JSON save.
pub fn shoulder_file(playerdata: &Path, uuid: Uuid) -> PathBuf {
    playerdata
        .join(DIRECTORY)
        .join("shoulders")
        .join(format!("{uuid}.nbt"))
}

/// Writes the parrots on a player's shoulders. An empty pair removes the file, so a released
/// parrot cannot come back on the next login.
pub fn save_shoulders(playerdata: Option<&Path>, uuid: Uuid, shoulders: &Shoulders) {
    let Some(playerdata) = playerdata else {
        return;
    };
    let path = shoulder_file(playerdata, uuid);
    if shoulders.is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "failed to remove a shoulder save")
            }
        }
        return;
    }
    let write = || -> std::io::Result<()> {
        let bytes =
            binary::write_compressed("shoulders", &shoulders.to_nbt(), binary::Compression::Gzip)
                .map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("nbt.tmp");
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, path)
    };
    if let Err(error) = write() {
        tracing::warn!(player = %uuid, %error, "failed to save shoulder parrots");
    }
}

/// Reads a player's shoulder parrots; a missing or corrupt file means empty shoulders.
pub fn load_shoulders(playerdata: Option<&Path>, uuid: Uuid) -> Shoulders {
    let Some(playerdata) = playerdata else {
        return Shoulders::default();
    };
    let path = shoulder_file(playerdata, uuid);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Shoulders::default();
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring an unreadable shoulder save");
            return Shoulders::default();
        }
    };
    match binary::read_compressed(&bytes, binary::Compression::Gzip) {
        Ok((_, tag)) => Shoulders::from_nbt(&tag),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring a corrupt shoulder save");
            Shoulders::default()
        }
    }
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

/// Every chunk of `world` that has an entity sidecar on disk, in coordinate order. A missing
/// directory or an unrelated file yields nothing for that entry.
pub fn saved_chunks(world: &Path) -> Vec<(i32, i32)> {
    let Ok(entries) = std::fs::read_dir(world.join(DIRECTORY)) else {
        return Vec::new();
    };
    let mut chunks: Vec<_> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let rest = name.strip_prefix("c.")?.strip_suffix(".nbt")?;
            let (x, z) = rest.split_once('.')?;
            Some((x.parse().ok()?, z.parse().ok()?))
        })
        .collect();
    chunks.sort_unstable();
    chunks
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
        Ok((_, tag)) => {
            let records = from_nbt(&tag);
            restore_rides(&tag, &records);
            records
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring a corrupt entity file");
            Vec::new()
        }
    }
}

/// Writes every entity of every region, grouped per chunk, for shutdown. Regions are consumed by
/// the scheduler shutdown, so this is called after the workers have stopped.
pub fn save_regions(world: &Path, regions: &mut [super::region::Region]) -> std::io::Result<()> {
    save_chunks(world, &drain_regions(regions))
}

/// Entity records grouped by the chunk they stand in.
pub type ChunkEntities = std::collections::BTreeMap<(i32, i32), Vec<EntityRecord>>;

/// Removes every entity from `regions` and groups them by chunk. Callers that must be able to
/// put the entities back when a later step fails keep the returned map.
pub fn drain_regions(regions: &mut [super::region::Region]) -> ChunkEntities {
    let mut chunks = ChunkEntities::new();
    for region in regions {
        for record in region.drain_entities() {
            chunks.entry(record.chunk()).or_default().push(record);
        }
    }
    chunks
}

/// Writes every chunk's sidecar, attempting all of them even after a failure so one bad file
/// does not strand the rest. Returns the first error.
pub fn save_chunks(world: &Path, chunks: &ChunkEntities) -> std::io::Result<()> {
    let mut first = None;
    for (&chunk, records) in chunks {
        if let Err(error) = save(world, chunk, records) {
            tracing::error!(%error, ?chunk, "failed to save chunk entities");
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}

/// Serializes a chunk's entities into the sidecar root compound.
pub fn to_nbt(records: &[EntityRecord]) -> Tag {
    let mut root = Compound::new();
    root.insert("DataVersion".into(), Tag::Int(DATA_VERSION));
    let entities = records
        .iter()
        .filter(|record| !is_runtime_only(record))
        .filter_map(|record| {
            let mut saved = entity_to_nbt(record).or_else(|| {
                tracing::warn!(
                    uuid = %record.data.uuid,
                    "skipping an entity that cannot be saved"
                );
                None
            })?;
            // Vanilla keeps passengers with their vehicle (`Passengers`); a sidecar per chunk
            // cannot nest across chunks, so the rider records which vehicle it sits on.
            if let Some(vehicle) = super::ride::vehicle_of(record.data.uuid) {
                saved.insert(RIDE_VEHICLE.into(), Tag::IntArray(uuid_words(vehicle)));
            }
            Some(saved)
        })
        .collect();
    root.insert("Entities".into(), Tag::List(List::Compound(entities)));
    Tag::Compound(root)
}

/// Entities that are never written. A fishing bobber is not serializable in vanilla and is
/// discarded with its chunk. A lingering potion's cloud entity is driven by a region-side
/// `PotionCloud` that is not saved, so reloading it would leave a cloud that never ends.
fn is_runtime_only(record: &EntityRecord) -> bool {
    record.data.bobber.is_some() || record.data.kind == ferrite_entity::potion::cloud_type()
}

/// Applies an updated save-NBT compound to a live entity in `store` on its owning region.
/// Returns `false` if the entity already died/unloaded or the compound fails validation.
pub(super) fn apply_entity_nbt(
    store: &mut ferrite_entity::EntityStore,
    uuid: Uuid,
    compound: &Compound,
) -> bool {
    let Some(key) = store
        .keys()
        .iter()
        .copied()
        .find(|&k| store.data(k).is_some_and(|d| d.uuid == uuid))
    else {
        return false;
    };
    if store.flags(key).is_some_and(|f| f.has(Flags::REMOVED)) {
        return false;
    }
    let Some(updated) = entity_from_nbt(compound) else {
        return false;
    };
    store.set_pos(key, updated.pos);
    store.set_vel(key, updated.vel);
    if let Some(data) = store.data_mut(key) {
        data.yaw = updated.data.yaw;
        data.pitch = updated.data.pitch;
        data.tags = updated.data.tags;
        data.custom_name = updated.data.custom_name;
        data.equipment = updated.data.equipment;
        if let Some(mob) = updated.data.mob {
            data.mob = Some(mob);
            ferrite_entity::mob::sync_state_metadata(data);
        }
    }
    true
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

/// Seats the riders of a loaded chunk on the vehicles their records name.
fn restore_rides(tag: &Tag, records: &[EntityRecord]) {
    let Tag::Compound(root) = tag else {
        return;
    };
    let Some(Tag::List(List::Compound(entities))) = root.get("Entities") else {
        return;
    };
    let loaded: std::collections::HashSet<Uuid> = records.iter().map(|r| r.data.uuid).collect();
    for entity in entities {
        if let (Some(rider), Some(vehicle)) =
            (uuid_field(entity, "UUID"), uuid_field(entity, RIDE_VEHICLE))
            && loaded.contains(&rider)
        {
            super::ride::restore(rider, vehicle);
        }
    }
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

/// A block position in vanilla's `BlockPos.CODEC` form: a list of three ints.
fn block_pos_tag([x, y, z]: [i32; 3]) -> Tag {
    Tag::List(List::Int(vec![x, y, z]))
}

/// A saved block position, accepting the codec list and the older int-array/compound forms.
fn block_pos(c: &Compound, key: &str) -> Option<[i32; 3]> {
    match c.get(key)? {
        Tag::List(List::Int(values)) if values.len() == 3 => {
            Some([values[0], values[1], values[2]])
        }
        Tag::IntArray(values) if values.len() == 3 => Some([values[0], values[1], values[2]]),
        Tag::Compound(pos) => Some([int(pos, "X")?, int(pos, "Y")?, int(pos, "Z")?]),
        _ => None,
    }
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

fn long(c: &Compound, key: &str) -> Option<i64> {
    match c.get(key) {
        Some(Tag::Long(value)) => Some(*value),
        Some(Tag::Int(value)) => Some(i64::from(*value)),
        Some(Tag::Short(value)) => Some(i64::from(*value)),
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
    if let Some([x, y, z]) = state.bed {
        c.insert("Bed".into(), Tag::IntArray(vec![x, y, z]));
    }
    c.insert("LastRestock".into(), Tag::Long(state.last_restock));
    c.insert(
        "RestocksToday".into(),
        Tag::Int(i32::from(state.restocks_today)),
    );
    c.insert("RestockDay".into(), Tag::Long(state.restock_day));
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
    state.bed = match c.get("Bed") {
        Some(Tag::IntArray(words)) if words.len() == 3 => Some([words[0], words[1], words[2]]),
        _ => None,
    };
    state.last_restock = long(c, "LastRestock").unwrap_or(-villager::RESTOCK_COOLDOWN_TICKS);
    state.restocks_today = int(c, "RestocksToday")
        .unwrap_or(0)
        .clamp(0, i32::from(villager::RESTOCKS_PER_DAY)) as u8;
    state.restock_day = long(c, "RestockDay").unwrap_or(-1);
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

pub(super) fn entity_to_nbt(record: &EntityRecord) -> Option<Compound> {
    let data = &record.data;
    let props = ferrite_data::entity_props::get(data.kind);
    let mut c = Compound::new();
    c.insert("id".into(), Tag::String(props.key.to_owned()));
    c.insert("UUID".into(), Tag::IntArray(uuid_words(data.uuid)));
    if let Some(name) = &data.custom_name {
        c.insert("CustomName".into(), Tag::String(name.clone()));
    }
    if !data.equipment.is_empty() && data.mob.is_none() {
        c.insert("equipment".into(), Tag::Compound(data.equipment.to_nbt()));
    }
    if let Some(dc) = data.equipment.drop_chances_to_nbt() {
        c.insert("drop_chances".into(), Tag::Compound(dc));
    }
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
        // The synched slot holds only an id and a count; the components live in the item state,
        // so the sidecar stores the stack as `full_stack` assembles it.
        let stack = ferrite_entity::item::full_stack(data);
        if let Some(nbt) = super::item_keep::to_nbt_keeping_item(&stack) {
            c.insert("Item".into(), Tag::Compound(nbt));
        }
        c.insert("Age".into(), Tag::Int(item.age));
        c.insert("PickupDelay".into(), Tag::Int(item.pickup_delay));
        c.insert("ItemHealth".into(), Tag::Int(item.health));
        c.insert(
            "UnlimitedLifetime".into(),
            Tag::Byte(item.unlimited_lifetime as i8),
        );
        c.insert("CanMobPickup".into(), Tag::Byte(item.can_mob_pickup as i8));
        c.insert(
            "CanPlayerPickup".into(),
            Tag::Byte(item.can_player_pickup as i8),
        );
        c.insert("WillAge".into(), Tag::Byte(item.will_age as i8));
        if let Some(target) = item.target {
            c.insert("PickupTarget".into(), Tag::IntArray(uuid_words(target)));
        }
        if let Some(thrower) = item.thrower {
            c.insert("Thrower".into(), Tag::IntArray(uuid_words(thrower)));
        }
    }

    if let Some(arrow) = &data.arrow {
        c.insert("damage".into(), Tag::Double(f64::from(arrow.damage)));
        c.insert("crit".into(), Tag::Byte(arrow.critical as i8));
        c.insert("life".into(), Tag::Int(arrow.age as i32));
        if let Some(owner) = arrow.owner {
            c.insert("owner".into(), Tag::IntArray(uuid_words(owner)));
        }
        if let Some(effect) = arrow.effect {
            let mut eff = Compound::new();
            eff.insert("id".into(), Tag::Int(effect.effect));
            eff.insert("duration".into(), Tag::Int(effect.duration));
            eff.insert("amplifier".into(), Tag::Byte(effect.amplifier as i8));
            c.insert("ArrowEffect".into(), Tag::Compound(eff));
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
        c.insert(
            "DragonPhase".into(),
            Tag::String(mob.dragon_phase.name().to_owned()),
        );
        c.insert(
            "DragonPhaseTicks".into(),
            Tag::Int(mob.dragon_phase_ticks as i32),
        );
        c.insert("PerchFlames".into(), Tag::Int(i32::from(mob.perch_flames)));
        if mob.kind == MobKind::EnderDragon {
            if mob.dragon_phase == DragonPhase::ChargePlayer {
                if let Some(target) = mob.dragon_charge_target
                    && target.x.is_finite()
                    && target.y.is_finite()
                    && target.z.is_finite()
                {
                    c.insert("DragonChargeTarget".into(), vec3_tag(target));
                }
                if mob.dragon_charge_recovery > 0 {
                    c.insert(
                        "DragonChargeRecovery".into(),
                        Tag::Int(mob.dragon_charge_recovery as i32),
                    );
                }
            }
            if mob.dragon_yaw_rate != 0.0 && mob.dragon_yaw_rate.is_finite() {
                c.insert("DragonYawRate".into(), Tag::Float(mob.dragon_yaw_rate));
            }
        }
        if let Some((name, props)) = mob.carried.and_then(ferrite_data::blocks::state) {
            // `Enderman.addAdditionalSaveData`: the carried block as a block state compound.
            let mut state = Compound::new();
            state.insert("Name".into(), Tag::String(name.to_owned()));
            if !props.is_empty() {
                let mut properties = Compound::new();
                for (key, value) in props {
                    properties.insert((*key).into(), Tag::String((*value).to_owned()));
                }
                state.insert("Properties".into(), Tag::Compound(properties));
            }
            c.insert("carriedBlockState".into(), Tag::Compound(state));
        }
        if mob.kind == MobKind::Creeper {
            c.insert("powered".into(), Tag::Byte(mob.powered as i8));
            if mob.ignited {
                c.insert("ignited".into(), Tag::Byte(1));
            }
        }
        if matches!(mob.kind, MobKind::Zombie | MobKind::Husk) && mob.in_water_time > 0 {
            c.insert("InWaterTime".into(), Tag::Int(i32::from(mob.in_water_time)));
        }
        if mob.kind == MobKind::Bee {
            // Vanilla `Bee.addAdditionalSaveData` field names.
            c.insert("HasNectar".into(), Tag::Byte(mob.has_nectar as i8));
            c.insert("HasStung".into(), Tag::Byte(mob.has_stung as i8));
            c.insert(
                "CropsGrownSincePollination".into(),
                Tag::Int(i32::from(mob.crops_grown)),
            );
            c.insert(
                "TicksSincePollination".into(),
                Tag::Int(i32::from(mob.ticks_since_pollination)),
            );
            c.insert(
                "CannotEnterHiveTicks".into(),
                Tag::Int(i32::from(mob.stay_out_of_hive)),
            );
            c.insert(
                "TicksSinceSting".into(),
                Tag::Int(i32::from(mob.sting_ticks)),
            );
            c.insert(
                "TicksWithoutNectar".into(),
                Tag::Int(i32::from(mob.ticks_without_nectar)),
            );
            if let Some(hive) = mob.hive_pos {
                c.insert("hive_pos".into(), block_pos_tag(hive));
            }
            if let Some(flower) = mob.flower_pos {
                c.insert("flower_pos".into(), block_pos_tag(flower));
            }
        }
        if mob.kind == MobKind::Chicken && mob.is_chicken_jockey {
            c.insert("IsChickenJockey".into(), Tag::Byte(1));
        }
        {
            let mut eq = data.equipment.to_nbt();
            for slot in ferrite_entity::EquipmentSlot::ALL {
                if !eq.contains_key(slot.nbt_key()) {
                    let idx = slot.as_index();
                    let item_id = if idx < ferrite_entity::mob::EQUIPMENT_SLOTS {
                        mob.equipment[idx]
                    } else if slot == ferrite_entity::EquipmentSlot::Body
                        && ferrite_entity::mob::can_use_equipment_slot(mob.kind, slot)
                    {
                        mob.armor
                    } else {
                        0
                    };
                    let item_name = if item_id != 0 {
                        ferrite_item::item_name(item_id)
                    } else if slot == ferrite_entity::EquipmentSlot::Saddle
                        && ferrite_entity::mob::can_use_equipment_slot(mob.kind, slot)
                        && mob.saddled
                    {
                        Some("minecraft:saddle")
                    } else {
                        None
                    };
                    if let Some(name) = item_name {
                        let mut stack = Compound::new();
                        stack.insert("id".into(), Tag::String(name.to_owned()));
                        stack.insert("count".into(), Tag::Int(1));
                        eq.insert(slot.nbt_key().into(), Tag::Compound(stack));
                    }
                }
            }
            if !eq.is_empty() {
                c.insert("equipment".into(), Tag::Compound(eq));
            }
        }
        if mob.kind == MobKind::Sheep {
            // Vanilla saves the legacy colour id as `Color` and the shorn flag as `Sheared`.
            c.insert("Color".into(), Tag::Byte(mob.wool_color as i8));
            c.insert("Sheared".into(), Tag::Byte(mob.sheared as i8));
        }
        if mob.kind == MobKind::Bogged && mob.sheared {
            c.insert("sheared".into(), Tag::Byte(1));
        }
        if mob.kind == MobKind::SnowGolem {
            c.insert("Pumpkin".into(), Tag::Byte((!mob.sheared) as i8));
        }
        if mob.kind == MobKind::Goat {
            // `Goat.addAdditionalSaveData` (26.3).
            c.insert("IsScreamingGoat".into(), Tag::Byte(mob.screaming as i8));
            c.insert("HasLeftHorn".into(), Tag::Byte(mob.has_left_horn as i8));
            c.insert("HasRightHorn".into(), Tag::Byte(mob.has_right_horn as i8));
        }
        if mob.kind == MobKind::IronGolem {
            // `IronGolem.addAdditionalSaveData` (26.3).
            c.insert("PlayerCreated".into(), Tag::Byte(mob.player_created as i8));
        }
        if mob.kind == MobKind::Fox {
            // `Fox.addAdditionalSaveData`: the `Trusted` list is a list of UUID strings (the
            // `EntityReference.CODEC` over `UUIDUtil.CODEC`), plus Sitting/Sleeping/Crouching.
            c.insert("Sleeping".into(), Tag::Byte(mob.fox_sleeping as i8));
            c.insert("Crouching".into(), Tag::Byte(mob.fox_crouching as i8));
            let trusted: Vec<String> = mob
                .trusted
                .iter()
                .flatten()
                .map(|uuid| uuid.to_string())
                .collect();
            if !trusted.is_empty() {
                c.insert("Trusted".into(), Tag::List(List::String(trusted)));
            }
            if let Some(v) = mob.variant {
                let name = if v == 1 { "snow" } else { "red" };
                c.insert("Type".into(), Tag::String(name.to_owned()));
            }
        }
        if mob.kind == MobKind::Rabbit
            && let Some(v) = mob.variant
        {
            c.insert("RabbitType".into(), Tag::Int(i32::from(v)));
        }
        if mob.kind == MobKind::Mooshroom
            && let Some(v) = mob.variant
        {
            let name = if v == 1 { "brown" } else { "red" };
            c.insert("Type".into(), Tag::String(name.to_owned()));
        }
        if matches!(
            mob.kind,
            MobKind::Frog | MobKind::Cow | MobKind::Pig | MobKind::Chicken
        ) && let Some(v) = mob.variant
        {
            let name = match v {
                0 => "minecraft:cold",
                2 => "minecraft:warm",
                _ => "minecraft:temperate",
            };
            c.insert("variant".into(), Tag::String(name.to_owned()));
        }
        c.insert("Age".into(), Tag::Int(mob.age));
        if let Some(owner) = mob.owner {
            c.insert("Owner".into(), Tag::IntArray(uuid_words(owner)));
        }
        if let Some(variant) = mob
            .variant
            .and_then(|v| ferrite_entity::variant::name(mob.kind, v))
        {
            c.insert("variant".into(), Tag::String(variant.to_owned()));
        }
        if let Some(index) = mob.sound_variant
            && let Some(name) = ferrite_entity::variant::sound_name(mob.kind, index)
        {
            c.insert("sound_variant".into(), Tag::String(name.to_owned()));
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
        c.insert("Anger".into(), Tag::Short(mob.anger as i16));
        if mob.kind == MobKind::Chicken {
            c.insert("EggLayTime".into(), Tag::Int(mob.egg_time as i32));
        }
        if mob.kind == MobKind::Turtle {
            // `Turtle.addAdditionalSaveData`: `home_pos` is a BlockPos int array and `has_egg`
            // is a boolean. The lay counter and chosen nest are transient vanilla state.
            if let Some(home) = mob.home {
                c.insert(
                    "home_pos".into(),
                    Tag::IntArray(vec![home[0], home[1], home[2]]),
                );
            }
            c.insert("has_egg".into(), Tag::Byte(mob.has_egg as i8));
        }
        if mob.kind == MobKind::Piglin
            && let Some(brain) = ferrite_entity::piglin::brain_to_nbt(mob.admiring)
        {
            c.insert("Brain".into(), Tag::Compound(brain));
        }
        if let Some(target) = mob.target {
            c.insert("Target".into(), Tag::IntArray(uuid_words(target)));
        }
    }

    if let Some(inv) = &data.piglin_inventory
        && !inv.is_empty()
    {
        c.insert("Inventory".into(), inv.to_nbt());
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
    if let Some(owner) = data.origin_owner {
        c.insert("owner".into(), Tag::IntArray(uuid_words(owner)));
    }
    if let Some(state) = data.block_state {
        c.insert("BlockState".into(), Tag::Int(i32::from(state)));
    }
    Some(c)
}

pub fn entity_from_nbt(c: &Compound) -> Option<EntityRecord> {
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
    if let Some(Tag::String(name)) = c.get("CustomName") {
        data.custom_name = Some(name.clone());
    }

    let item_stack = c
        .get("Item")
        .and_then(|tag| match tag {
            Tag::Compound(item) => ferrite_item::ItemStack::from_nbt(item).ok(),
            _ => None,
        })
        .filter(|stack| !stack.is_empty())
        .or_else(|| {
            (props.key == "minecraft:item")
                .then(|| ferrite_item::ItemStack::from_name("minecraft:stone"))
                .flatten()
        });
    if let Some(stack) = item_stack {
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
            components: stack.components.clone(),
            age: int(c, "Age").unwrap_or(0),
            pickup_delay: int(c, "PickupDelay").unwrap_or(0),
            health: int(c, "ItemHealth").unwrap_or(5),
            target: uuid_field(c, "PickupTarget"),
            thrower: uuid_field(c, "Thrower"),
            unlimited_lifetime: boolean(c, "UnlimitedLifetime"),
            can_mob_pickup: !c.contains_key("CanMobPickup") || boolean(c, "CanMobPickup"),
            can_player_pickup: !c.contains_key("CanPlayerPickup") || boolean(c, "CanPlayerPickup"),
            will_age: !c.contains_key("WillAge") || boolean(c, "WillAge"),
        });
    }

    if c.contains_key("damage") || ferrite_entity::arrow::kind_of(&data).is_some() {
        let default_damage = match ferrite_entity::arrow::kind_of(&data) {
            Some(ferrite_entity::arrow::Kind::Trident) => ferrite_entity::arrow::TRIDENT_DAMAGE,
            Some(
                ferrite_entity::arrow::Kind::Arrow | ferrite_entity::arrow::Kind::SpectralArrow,
            ) => 2.0,
            Some(ferrite_entity::arrow::Kind::LlamaSpit) => 1.0,
            _ => 0.0,
        };
        let effect = match c.get("ArrowEffect") {
            Some(Tag::Compound(eff)) => Some(ferrite_entity::effect::MobEffectInstance::new(
                int(eff, "id").unwrap_or(0),
                int(eff, "duration").unwrap_or(0),
                int(eff, "amplifier").unwrap_or(0),
            )),
            _ => None,
        };
        data.arrow = Some(ArrowState {
            damage: float(c, "damage").unwrap_or(default_damage),
            owner: uuid_field(c, "owner").or_else(|| uuid_field(c, "Owner")),
            critical: boolean(c, "crit"),
            age: int(c, "life").unwrap_or(0).max(0) as u32,
            channeling: boolean(c, "channeling"),
            effect,
        });
    }

    let resolved_mob_kind = match c.get("MobKind") {
        Some(Tag::String(kind)) => {
            let parsed = MobKind::from_name(kind);
            if parsed.is_none() {
                tracing::warn!(kind, "ignoring an unknown mob kind");
            }
            parsed
        }
        _ => MobKind::from_name(props.key),
    };
    if let Some(kind) = resolved_mob_kind {
        let mut mob = MobState::new(kind);
        data.metadata.define_mob(kind);
        mob.health = float(c, "Health").unwrap_or(mob.health);
        mob.hurt_time = int(c, "HurtTime").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
        mob.no_action_time = int(c, "NoActionTime").unwrap_or(0).max(0) as u32;
        mob.fall_distance = float(c, "FallDistance").unwrap_or(0.0);
        mob.despawns = boolean(c, "Despawns");
        mob.loot_dropped = boolean(c, "LootDropped");
        mob.no_ai = boolean(c, "NoAI");
        mob.wander_timer = int(c, "WanderTimer").unwrap_or(0).max(0) as u32;
        mob.dragon_phase = match c.get("DragonPhase") {
            Some(Tag::String(name)) => DragonPhase::from_name(name).unwrap_or(DragonPhase::Holding),
            _ => DragonPhase::Holding,
        };
        mob.dragon_phase_ticks = int(c, "DragonPhaseTicks").unwrap_or(0).max(0) as u32;
        mob.perch_flames = int(c, "PerchFlames").unwrap_or(0).clamp(0, 255) as u8;
        if mob.kind == MobKind::EnderDragon {
            if c.get("NoPhysics").is_none() {
                flags.set(Flags::NO_PHYSICS, true);
            }
            if mob.dragon_phase == DragonPhase::ChargePlayer {
                mob.dragon_charge_target = vec3(c, "DragonChargeTarget")
                    .filter(|t| t.x.is_finite() && t.y.is_finite() && t.z.is_finite());
                mob.dragon_charge_recovery =
                    int(c, "DragonChargeRecovery").unwrap_or(0).max(0) as u32;
            }
            if let Some(rate) = float(c, "DragonYawRate").filter(|r| r.is_finite()) {
                mob.dragon_yaw_rate = rate;
            }
        }
        mob.wander_yaw = float(c, "WanderYaw").unwrap_or(0.0);
        mob.moving = boolean(c, "Moving");
        mob.attack_cooldown = int(c, "AttackCooldown")
            .unwrap_or(0)
            .clamp(0, i16::MAX as i32) as u16;
        mob.panic = int(c, "Panic").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
        mob.swell = int(c, "Swell").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
        mob.bow_timer = int(c, "BowTimer").unwrap_or(0).clamp(0, i16::MAX as i32) as u16;
        mob.anger = int(c, "Anger").unwrap_or(0).clamp(0, i32::from(u16::MAX)) as u16;
        mob.egg_time = int(c, "EggLayTime")
            .unwrap_or(ferrite_entity::mob::CHICKEN_EGG_MIN as i32)
            .max(0) as u32;
        mob.target = uuid_field(c, "Target");
        mob.age = int(c, "Age").unwrap_or(0);
        if kind == MobKind::Turtle {
            let fallback = [
                pos.x.floor() as i32,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
            ];
            // `Turtle.readAdditionalSaveData` falls back to the current block position when
            // `home_pos` is absent. A malformed tag is treated the same way, never panicking.
            mob.home = match c.get("home_pos") {
                Some(Tag::IntArray(home))
                    if home.len() == 3
                        && home
                            .iter()
                            .all(|value| (-30_000_000..=30_000_000).contains(value)) =>
                {
                    Some([home[0], home[1], home[2]])
                }
                Some(_) => {
                    tracing::warn!(
                        uuid = %data.uuid,
                        "turtle home_pos is malformed; using the current position"
                    );
                    Some(fallback)
                }
                None => Some(fallback),
            };
            mob.has_egg = boolean(c, "has_egg");
            mob.laying_egg = false;
            mob.lay_egg_counter = 0;
            mob.lay_egg_target = None;
            ferrite_entity::turtle::sync_metadata(&mut data.metadata, &mob);
        }
        mob.love = int(c, "InLove").unwrap_or(0).clamp(0, i32::from(u16::MAX)) as u16;
        mob.owner = uuid_field(c, "Owner");
        mob.variant = match c.get("variant") {
            Some(Tag::String(name)) => ferrite_entity::variant::index(mob.kind, name),
            _ => None,
        };
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
        if let Some(Tag::Compound(state)) = c.get("carriedBlockState")
            && let Some(Tag::String(name)) = state.get("Name")
        {
            let props: Vec<(&str, &str)> = match state.get("Properties") {
                Some(Tag::Compound(p)) => p
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Tag::String(v) => Some((k.as_str(), v.as_str())),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            mob.carried = ferrite_data::blocks::state_id(name, &props);
        }
        if kind == MobKind::Enderman {
            // The metadata is rebuilt from the saved state so a player who starts
            // tracking right after a reload sees the carried block.
            ferrite_entity::enderman::define_metadata(&mut data.metadata);
            data.metadata.set(
                ferrite_entity::enderman::CARRIED_BLOCK_INDEX,
                ferrite_protocol::packets::play::MetaValue::OptionalBlockState(mob.carried),
            );
        }
        if kind == MobKind::Creeper {
            mob.powered = boolean(c, "powered");
            mob.ignited = boolean(c, "ignited");
            ferrite_entity::creeper::define_metadata(&mut data.metadata);
            data.metadata.set(
                ferrite_entity::creeper::POWERED_INDEX,
                ferrite_protocol::packets::play::MetaValue::Boolean(mob.powered),
            );
            data.metadata.set(
                ferrite_entity::creeper::IGNITED_INDEX,
                ferrite_protocol::packets::play::MetaValue::Boolean(mob.ignited),
            );
        }
        if kind.is_zombie_like() {
            mob.in_water_time = match c.get("InWaterTime") {
                Some(Tag::Int(t)) if *t > 0 => (*t).min(i32::from(u16::MAX)) as u16,
                _ => 0,
            };
            ferrite_entity::mob::define_zombie_metadata(&mut data.metadata, &mob);
        }
        if kind == MobKind::EnderDragon {
            ferrite_entity::dragon::define_metadata(&mut data.metadata, mob.dragon_phase);
        }
        if kind.is_spider_like() {
            ferrite_entity::spider::define_metadata(&mut data.metadata);
        }
        if kind == MobKind::Pig {
            ferrite_entity::pig::define_metadata(&mut data.metadata);
        }
        if kind == MobKind::Chicken {
            mob.is_chicken_jockey = boolean(c, "IsChickenJockey");
        }
        if let Some(Tag::Compound(eq)) = c.get("equipment") {
            data.equipment = ferrite_entity::Equipment::from_nbt(eq);
            data.equipment.clear_dirty();
            for slot in ferrite_entity::EquipmentSlot::ALL {
                if !ferrite_entity::mob::can_use_equipment_slot(kind, slot) {
                    data.equipment.set(slot, None);
                    data.equipment.clear_dirty();
                    continue;
                }
                let idx = slot.as_index();
                let item_id = if let Some(stack) = data.equipment.get(slot) {
                    Some(stack.item)
                } else if let Some(Tag::Compound(stack)) = eq.get(slot.nbt_key())
                    && let Some(Tag::String(item_name)) = stack.get("id")
                {
                    ferrite_item::item_id(item_name)
                } else {
                    None
                };
                if let Some(item_id) = item_id {
                    if idx < ferrite_entity::mob::EQUIPMENT_SLOTS {
                        mob.equipment[idx] = item_id;
                    } else if slot == ferrite_entity::EquipmentSlot::Body && mob.armor == 0 {
                        mob.armor = item_id;
                        if let Some(name) = ferrite_item::item_name(item_id) {
                            mob.armor_points = ferrite_entity::horse::armor_points(name);
                        }
                    } else if slot == ferrite_entity::EquipmentSlot::Saddle {
                        mob.saddled = true;
                    }
                }
            }
        }
        if let Some(Tag::Compound(dc)) = c.get("drop_chances") {
            data.equipment.drop_chances_from_nbt(dc);
        }
        if kind == MobKind::Sheep {
            mob.wool_color = match c.get("Color") {
                Some(Tag::Byte(id)) => (*id & 15) as u8,
                _ => 0,
            };
            mob.sheared = boolean(c, "Sheared");
            ferrite_entity::sheep::define_metadata(&mut data.metadata, &mob);
        }
        if kind == MobKind::Bogged {
            mob.sheared = boolean(c, "sheared");
        }
        if kind == MobKind::SnowGolem {
            // Vanilla `SnowGolem` defaults `Pumpkin` to true when absent.
            mob.sheared = c.contains_key("Pumpkin") && !boolean(c, "Pumpkin");
        }
        if kind == MobKind::Goat {
            // `Goat.IsScreamingGoat` defaults false; both horns default true.
            mob.screaming = boolean(c, "IsScreamingGoat");
            // `getBooleanOr(name, true)`: a missing or non-boolean key leaves the horn on.
            mob.has_left_horn =
                !matches!(c.get("HasLeftHorn"), Some(Tag::Byte(value)) if *value == 0);
            mob.has_right_horn =
                !matches!(c.get("HasRightHorn"), Some(Tag::Byte(value)) if *value == 0);
        }
        if kind == MobKind::IronGolem {
            mob.player_created = boolean(c, "PlayerCreated");
        }
        if kind == MobKind::Fox {
            mob.fox_sleeping = boolean(c, "Sleeping");
            mob.fox_crouching = boolean(c, "Crouching");
            mob.trusted = [None; 2];
            if let Some(Tag::List(List::String(trusted))) = c.get("Trusted") {
                for value in trusted {
                    if let Ok(uuid) = Uuid::parse_str(value) {
                        ferrite_entity::fox::insert_trusted(&mut mob.trusted, uuid);
                    }
                }
            }
            if let Some(Tag::String(ty)) = c.get("Type") {
                mob.variant = Some(u8::from(ty == "snow"));
            }
        }
        if kind == MobKind::Rabbit
            && let Some(ty) = int(c, "RabbitType")
            && (0..=255).contains(&ty)
        {
            mob.variant = Some(ty as u8);
        }
        if kind == MobKind::Mooshroom
            && let Some(Tag::String(ty)) = c.get("Type")
        {
            mob.variant = Some(u8::from(ty == "brown"));
        }
        if matches!(
            kind,
            MobKind::Frog | MobKind::Cow | MobKind::Pig | MobKind::Chicken
        ) && let Some(Tag::String(v)) = c.get("variant")
        {
            mob.variant = match v.strip_prefix("minecraft:").unwrap_or(v) {
                "cold" => Some(0),
                "temperate" => Some(1),
                "warm" => Some(2),
                _ => None,
            };
        }
        if let Some(Tag::String(name)) = c.get("sound_variant") {
            mob.sound_variant = ferrite_entity::variant::sound_index(kind, name);
        }
        if kind == MobKind::Piglin {
            if c.get("Age").is_none() && boolean(c, "IsBaby") {
                mob.age = ferrite_entity::mob::PIGLIN_BABY_AGE;
            }
            if let Some(Tag::Compound(brain)) = c.get("Brain") {
                mob.admiring = ferrite_entity::piglin::admiring_from_nbt(brain);
            }
        }
        if kind == MobKind::Bee {
            mob.has_nectar = boolean(c, "HasNectar");
            mob.has_stung = boolean(c, "HasStung");
            mob.hive_pos = block_pos(c, "hive_pos");
            mob.flower_pos = block_pos(c, "flower_pos");
            mob.crops_grown = int(c, "CropsGrownSincePollination")
                .unwrap_or(0)
                .clamp(0, i32::from(ferrite_entity::bee::MAX_CROPS_GROWN))
                as u8;
            mob.ticks_since_pollination = int(c, "TicksSincePollination")
                .unwrap_or(0)
                .clamp(0, i32::from(u16::MAX)) as u16;
            mob.stay_out_of_hive = int(c, "CannotEnterHiveTicks")
                .unwrap_or(0)
                .clamp(0, i32::from(u16::MAX)) as u16;
            mob.sting_ticks = int(c, "TicksSinceSting")
                .unwrap_or(0)
                .clamp(0, i32::from(u16::MAX)) as u16;
            mob.ticks_without_nectar = int(c, "TicksWithoutNectar")
                .unwrap_or(0)
                .clamp(0, i32::from(u16::MAX)) as u16;
        }
        data.mob = Some(mob);
        if kind == MobKind::Bee {
            let flags = data
                .mob
                .as_ref()
                .map(ferrite_entity::bee::metadata_flags)
                .unwrap_or(0);
            data.metadata.set(
                ferrite_entity::metadata::bee::FLAGS,
                ferrite_protocol::packets::play::MetaValue::Byte(flags),
            );
        }
        if kind == MobKind::Piglin {
            data.piglin_inventory = Some(
                c.get("Inventory")
                    .map(ferrite_entity::piglin::PiglinInventory::from_nbt)
                    .unwrap_or_default(),
            );
        }
    }

    if data.mob.is_none() {
        if let Some(Tag::Compound(eq)) = c.get("equipment") {
            data.equipment = ferrite_entity::Equipment::from_nbt(eq);
            data.equipment.clear_dirty();
        }
        if let Some(Tag::Compound(dc)) = c.get("drop_chances") {
            data.equipment.drop_chances_from_nbt(dc);
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
        if kind == MobKind::Villager {
            ferrite_entity::villager::define_metadata(&mut data.metadata, &state);
        } else {
            ferrite_entity::villager::define_abstract_metadata(&mut data.metadata);
        }
        data.villager = Some(state);
    }

    if data.mob.is_some() {
        ferrite_entity::mob::sync_state_metadata(&mut data);
    }
    if let (Some(arrow_state), Some(arrow_kind)) =
        (data.arrow, ferrite_entity::arrow::kind_of(&data))
    {
        ferrite_entity::arrow::define_projectile_metadata(
            &mut data.metadata,
            arrow_kind,
            arrow_state.critical,
        );
    }

    if c.contains_key("OrbCount") || props.key == "minecraft:experience_orb" {
        // `new_orb` sets up the synched value accessor; the saved state overrides the rest.
        let value = int(c, "OrbValue")
            .or_else(|| int(c, "Value"))
            .unwrap_or(1)
            .max(0);
        let mut record = ferrite_entity::experience::new_orb(pos, value);
        record.data.uuid = data.uuid;
        record.vel = vel;
        record.flags = flags;
        record.data.tick_count = data.tick_count;
        record.data.tags = data.tags;
        record.data.orb = Some(OrbState {
            age: int(c, "OrbAge").or_else(|| int(c, "Age")).unwrap_or(0),
            count: int(c, "OrbCount")
                .or_else(|| int(c, "Count"))
                .unwrap_or(1)
                .max(1),
        });
        record.data.metadata.clear_dirty();
        return Some(record);
    }

    data.fuse = int(c, "fuse")
        .or_else(|| int(c, "Fuse"))
        .or_else(|| (props.key == "minecraft:tnt").then_some(ferrite_entity::tnt::FUSE));
    if data.arrow.is_none() {
        data.origin_owner = uuid_field(c, "owner").or_else(|| uuid_field(c, "Owner"));
    }
    if let Some(fuse) = data.fuse {
        use ferrite_protocol::packets::play::MetaValue;
        let tnt_state = ferrite_data::blocks::default_state("minecraft:tnt").unwrap_or(0);
        data.metadata.define(
            ferrite_entity::metadata::tnt::FUSE,
            MetaValue::Int(ferrite_entity::tnt::FUSE),
        );
        data.metadata.define(
            ferrite_entity::metadata::tnt::BLOCK_STATE,
            MetaValue::BlockState(tnt_state),
        );
        data.metadata
            .set(ferrite_entity::metadata::tnt::FUSE, MetaValue::Int(fuse));
    }
    data.block_state = int(c, "BlockState")
        .and_then(|state| u16::try_from(state).ok())
        .or_else(|| {
            let Tag::Compound(state) = c.get("BlockState")? else {
                return None;
            };
            let Some(Tag::String(name)) = state.get("Name") else {
                return None;
            };
            let props_list: Vec<(&str, &str)> = match state.get("Properties") {
                Some(Tag::Compound(p)) => p
                    .iter()
                    .filter_map(|(k, v)| match v {
                        Tag::String(v) => Some((k.as_str(), v.as_str())),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            ferrite_data::blocks::state_id(name, &props_list)
        })
        .or_else(|| {
            (props.key == "minecraft:falling_block")
                .then(|| ferrite_data::blocks::default_state("minecraft:sand"))
                .flatten()
        });
    if data.block_state.is_some() {
        use ferrite_protocol::packets::play::MetaValue;
        let start_pos = ferrite_protocol::types::BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        )
        .pack();
        data.metadata.define(
            ferrite_entity::metadata::falling_block::START_POS,
            MetaValue::BlockPos(0),
        );
        data.metadata.set(
            ferrite_entity::metadata::falling_block::START_POS,
            MetaValue::BlockPos(start_pos),
        );
    }
    if props.key == "minecraft:firework_rocket" && data.firework.is_none() {
        let mut rng = ferrite_entity::Rng::new(data.uuid.as_u128() as u64);
        let fw = ferrite_entity::firework::new_firework(pos, 1, 0, None, &mut rng);
        data.metadata = fw.data.metadata;
        data.firework = fw.data.firework;
    }
    if props.key == "minecraft:eye_of_ender" && data.eye.is_none() {
        let mut rng = ferrite_entity::Rng::new(data.uuid.as_u128() as u64);
        let eye = ferrite_entity::eye::new_eye(pos, &mut rng);
        data.metadata = eye.data.metadata;
        data.eye = eye.data.eye;
    }
    data.metadata.clear_dirty();
    Some(EntityRecord {
        pos,
        vel,
        flags,
        data,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn saved_chunks_lists_only_entity_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        assert!(saved_chunks(dir.path()).is_empty(), "no directory yet");
        let folder = dir.path().join(DIRECTORY);
        std::fs::create_dir_all(&folder).unwrap();
        for name in [
            "c.3.-2.nbt",
            "c.-1.0.nbt",
            "c.1.nbt",
            "c.x.1.nbt",
            "notes.txt",
            "c.4.4.nbt.tmp",
        ] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        assert_eq!(saved_chunks(dir.path()), vec![(-1, 0), (3, -2)]);
    }

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
    fn a_dropped_item_keeps_its_components_through_the_entity_save() {
        use ferrite_item::{Component, component_ids};
        let mut rng = Rng::default();
        let mut shulker = ferrite_item::ItemStack::from_name("lime_shulker_box").unwrap();
        shulker.components.insert(
            component_ids::CONTAINER,
            Component::Container(vec![
                ferrite_item::ItemStack::from_name("diamond")
                    .unwrap()
                    .copy_with_count(7),
            ]),
        );
        shulker.components.insert(
            component_ids::CUSTOM_NAME,
            Component::CustomName(ferrite_text::Component::text("Vorräte ✓")),
        );
        let record = ferrite_entity::item::new_item_stack(
            Vec3::new(1.5, 65.0, 2.5),
            Vec3::ZERO,
            &shulker,
            &mut rng,
        );
        let loaded = from_nbt(&to_nbt(&[record]));
        assert_eq!(loaded.len(), 1);
        assert_eq!(ferrite_entity::item::full_stack(&loaded[0].data), shulker);
    }

    #[test]
    fn a_saved_rider_is_seated_on_its_vehicle_again_after_the_chunk_reloads() {
        let _turn = super::super::ride::test_turn();
        let world = temp_world("rider-persists");
        let rider = mob_record();
        let rider_uuid = rider.data.uuid;
        let vehicle = Uuid::from_u128(0x7e1d_0001);
        let other = mob_record();
        super::super::ride::mount(rider_uuid, vehicle, false).unwrap();

        // Saving writes the link, the server stops, and the next start has an empty ride table.
        save(&world, (0, 0), &[rider.clone(), other.clone()]).unwrap();
        super::super::ride::forget(rider_uuid);
        assert_eq!(super::super::ride::vehicle_of(rider_uuid), None);

        let loaded = load(&world, (0, 0));
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            super::super::ride::vehicle_of(rider_uuid),
            Some(vehicle),
            "the saved ride is back"
        );
        assert_eq!(
            super::super::ride::vehicle_of(other.data.uuid),
            None,
            "a mob that was not riding stays unmounted"
        );

        // A rider that dismounted before the next save is written without a link.
        super::super::ride::dismount(rider_uuid);
        save(&world, (0, 0), &[rider]).unwrap();
        load(&world, (0, 0));
        assert_eq!(super::super::ride::vehicle_of(rider_uuid), None);
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn nbt_round_trip_keeps_mob_item_arrow_and_tnt_state() {
        let mut rng = Rng::default();
        let mob = mob_record();
        let item = item_record();
        let mut arrow = arrow::new_arrow(
            Vec3::new(0.5, 64.0, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            6.0,
            Some(Uuid::from_u128(7)),
            &mut rng,
        );
        arrow = arrow::with_effect(
            arrow,
            ferrite_entity::effect::MobEffectInstance::new(
                ferrite_entity::effect::Effect::Slowness.id(),
                arrow::STRAY_SLOWNESS_TICKS,
                0,
            ),
        );
        let tnt = tnt::new_primed_tnt(Vec3::new(5.5, 64.0, 5.5), 80);
        let records = vec![mob.clone(), item, arrow, tnt];

        let tag = to_nbt(&records);
        let loaded = from_nbt(&tag);
        assert_eq!(loaded.len(), records.len());
        assert_eq!(
            loaded[2].data.arrow.as_ref().unwrap().effect,
            Some(ferrite_entity::effect::MobEffectInstance::new(
                ferrite_entity::effect::Effect::Slowness.id(),
                arrow::STRAY_SLOWNESS_TICKS,
                0,
            ))
        );

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
    fn bobbers_and_lingering_clouds_are_never_saved_but_other_entities_are() {
        let bobber = ferrite_entity::bobber::cast(
            Uuid::from_u128(5),
            9,
            Vec3::new(1.5, 65.0, 1.5),
            0.0,
            0.0,
            0,
            &mut ferrite_entity::Rng::new(1),
        );
        let cloud = ferrite_entity::potion::PotionCloud::lingering(Vec3::new(2.5, 64.0, 2.5), &[])
            .spawn_record();
        let cow = mob_record();
        let cow_uuid = cow.data.uuid;
        let saved = from_nbt(&to_nbt(&[bobber, cloud, cow]));
        // Vanilla does not serialize bobbers, and a cloud whose region state is not saved
        // would come back as a bare cloud that never ends.
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].data.uuid, cow_uuid);
    }

    #[test]
    fn a_dragon_phase_survives_a_save() {
        let mut record = mob::new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.5, 90.0, 0.5),
            &mut Rng::default(),
        );
        {
            let state = record.data.mob.as_mut().unwrap();
            state.dragon_phase = DragonPhase::LandingApproach;
            state.dragon_phase_ticks = 137;
        }
        let loaded = from_nbt(&to_nbt(&[record]));
        let state = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::LandingApproach);
        assert_eq!(state.dragon_phase_ticks, 137);
    }

    #[test]
    fn every_perched_subphase_survives_a_save_with_its_timer() {
        for phase in [
            DragonPhase::Perched,
            DragonPhase::PerchAttacking,
            DragonPhase::PerchFlaming,
        ] {
            let mut record = mob::new_mob(
                MobKind::EnderDragon,
                Vec3::new(0.5, 68.0, 0.5),
                &mut Rng::default(),
            );
            {
                let state = record.data.mob.as_mut().unwrap();
                state.dragon_phase = phase;
                state.dragon_phase_ticks = 77;
            }
            let loaded = from_nbt(&to_nbt(&[record]));
            let state = loaded[0].data.mob.as_ref().unwrap();
            assert_eq!(
                (state.dragon_phase, state.dragon_phase_ticks),
                (phase, 77),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn a_charging_dragon_persists_its_target_and_recovery_across_save_and_reload() {
        let mut record = mob::new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.0, 68.0, 0.0),
            &mut Rng::default(),
        );
        let charge_target = Vec3::new(80.5, 72.0, -14.25);
        {
            let state = record.data.mob.as_mut().unwrap();
            state.dragon_phase = DragonPhase::ChargePlayer;
            state.dragon_phase_ticks = 19;
            state.dragon_charge_target = Some(charge_target);
            state.dragon_charge_recovery = 4;
            state.dragon_yaw_rate = -3.25;
        }
        let mut loaded = from_nbt(&to_nbt(&[record]));
        let reloaded = loaded.pop().expect("reloaded dragon");
        assert!(reloaded.flags.has(Flags::NO_PHYSICS));
        {
            let state = reloaded.data.mob.as_ref().unwrap();
            assert_eq!(state.dragon_phase, DragonPhase::ChargePlayer);
            assert_eq!(state.dragon_phase_ticks, 19);
            assert_eq!(state.dragon_charge_target, Some(charge_target));
            assert_eq!(state.dragon_charge_recovery, 4);
            assert_eq!(state.dragon_yaw_rate, -3.25);
        }

        // Resuming flight continues the remaining recovery ticks (4..=10 -> 7 ticks) and then
        // exits cleanly to Holding instead of aborting on the first tick.
        let mut store = ferrite_entity::EntityStore::new(4);
        let key = store.insert(reloaded);
        let mut rng = Rng::new(7);
        for expected_recovery in 5..=ferrite_entity::dragon::CHARGE_RECOVERY_TICKS {
            ferrite_entity::dragon::fly(&mut store, key, &[], &mut rng, &mut Vec::new());
            let state = mob::state(&store, key).unwrap();
            assert_eq!(state.dragon_phase, DragonPhase::ChargePlayer);
            assert_eq!(state.dragon_charge_recovery, expected_recovery);
        }
        ferrite_entity::dragon::fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Holding);
        assert_eq!(state.dragon_charge_target, None);
        assert_eq!(state.dragon_charge_recovery, 0);
    }

    #[test]
    fn a_charging_dragon_saved_without_a_target_aborts_to_holding_on_reload() {
        let mut record = mob::new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.0, 68.0, 0.0),
            &mut Rng::default(),
        );
        {
            let state = record.data.mob.as_mut().unwrap();
            state.dragon_phase = DragonPhase::ChargePlayer;
            state.dragon_charge_target = Some(Vec3::new(60.0, 70.0, 0.0));
        }
        let mut nbt = to_nbt(&[record]);
        // Strip DragonChargeTarget and NoPhysics to simulate an older or vanilla-style save.
        let Tag::Compound(root) = &mut nbt else {
            panic!("expected root compound");
        };
        let Tag::List(List::Compound(entities)) = root.get_mut("Entities").unwrap() else {
            panic!("expected Entities list");
        };
        assert!(entities[0].shift_remove("DragonChargeTarget").is_some());
        assert!(entities[0].shift_remove("NoPhysics").is_some());
        let mut loaded = from_nbt(&nbt);
        let reloaded = loaded.pop().expect("reloaded dragon");
        assert!(
            reloaded.flags.has(Flags::NO_PHYSICS),
            "older dragon saves default to vanilla noPhysics = true"
        );
        assert_eq!(
            reloaded.data.mob.as_ref().unwrap().dragon_charge_target,
            None
        );

        let mut store = ferrite_entity::EntityStore::new(4);
        let key = store.insert(reloaded);
        let mut rng = Rng::new(9);
        ferrite_entity::dragon::fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(
            mob::state(&store, key).unwrap().dragon_phase,
            DragonPhase::Holding
        );
    }

    #[test]
    fn a_strafing_dragon_persists_its_target_uuid_across_save_and_reload() {
        let target_id = uuid::Uuid::from_u128(0x00D8_A601);
        let mut record = mob::new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.0, 90.0, 0.0),
            &mut Rng::default(),
        );
        {
            let state = record.data.mob.as_mut().unwrap();
            state.dragon_phase = DragonPhase::Strafe;
            state.dragon_phase_ticks = 12;
            state.target = Some(target_id);
        }
        let loaded = from_nbt(&to_nbt(&[record]));
        let state = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Strafe);
        assert_eq!(state.dragon_phase_ticks, 12);
        assert_eq!(state.target, Some(target_id));
    }

    #[test]
    fn a_sheep_keeps_its_colour_and_shorn_wool_across_a_save() {
        let mut record = mob::new_mob(
            MobKind::Sheep,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        {
            let state = record.data.mob.as_mut().unwrap();
            state.wool_color = 11;
            state.sheared = true;
        }
        let loaded = from_nbt(&to_nbt(&[record]));
        let data = &loaded[0].data;
        let state = data.mob.as_ref().unwrap();
        assert_eq!((state.wool_color, state.sheared), (11, true));
        // The synched wool byte is rebuilt from the saved state.
        assert!(matches!(
            data.metadata.get(ferrite_entity::sheep::WOOL_INDEX),
            Some(ferrite_protocol::packets::play::MetaValue::Byte(byte)) if *byte == 11 | 0x10
        ));
    }

    #[test]
    fn an_angry_enderman_keeps_its_anger_and_target_across_a_save() {
        let mut record = mob::new_mob(
            MobKind::Enderman,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let target = Uuid::from_u128(0xa11);
        {
            let state = record.data.mob.as_mut().unwrap();
            state.anger = mob::ENDERMAN_ANGER_TICKS;
            state.target = Some(target);
        }
        let loaded = from_nbt(&to_nbt(&[record]));
        let state = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!(state.anger, mob::ENDERMAN_ANGER_TICKS);
        assert_eq!(state.target, Some(target));
    }

    #[test]
    fn an_enderman_keeps_its_carried_block_state_across_a_save() {
        let mut record = mob::new_mob(
            MobKind::Enderman,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let log = ferrite_data::blocks::state_id("minecraft:oak_log", &[("axis", "z")]).unwrap();
        record.data.mob.as_mut().unwrap().carried = Some(log);
        let loaded = from_nbt(&to_nbt(&[record]));
        assert_eq!(loaded[0].data.mob.as_ref().unwrap().carried, Some(log));
        assert_eq!(
            loaded[0]
                .data
                .metadata
                .get(ferrite_entity::enderman::CARRIED_BLOCK_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::OptionalBlockState(Some(log)))
        );
        // An enderman carrying nothing saves no compound at all.
        let empty = mob::new_mob(
            MobKind::Enderman,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let loaded = from_nbt(&to_nbt(&[empty]));
        assert_eq!(loaded[0].data.mob.as_ref().unwrap().carried, None);
        assert_eq!(
            loaded[0]
                .data
                .metadata
                .get(ferrite_entity::enderman::CARRIED_BLOCK_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::OptionalBlockState(None))
        );
    }

    #[test]
    fn a_charged_creeper_stays_charged_across_a_save() {
        let mut record = mob::new_mob(
            MobKind::Creeper,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        record.data.mob.as_mut().unwrap().powered = true;
        let loaded = from_nbt(&to_nbt(&[record]));
        let data = &loaded[0].data;
        assert!(data.mob.as_ref().unwrap().powered);
        assert_eq!(
            data.metadata.get(ferrite_entity::creeper::POWERED_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::Boolean(true))
        );
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
            state.bed = Some([4, 64, 5]);
            state.last_restock = 4321;
            state.restocks_today = 1;
            state.restock_day = 2;
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
        assert_eq!(state.bed, Some([4, 64, 5]));
        assert_eq!(state.last_restock, 4321);
        assert_eq!(state.restocks_today, 1);
        assert_eq!(state.restock_day, 2);
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
    fn a_chested_equine_keeps_its_pack_through_a_save() {
        let mut record = ferrite_entity::mob::new_mob(
            MobKind::Donkey,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let uuid = record.data.uuid;
        {
            let state = record.data.mob.as_mut().unwrap();
            state.owner = Some(Uuid::from_u128(6));
            state.chest = true;
            state.saddled = true;
        }
        let mut items = super::super::equine_chest::load(uuid, 15);
        items[4] = ferrite_item::ItemStack::from_name("minecraft:bread")
            .unwrap()
            .copy_with_count(3);
        super::super::equine_chest::store(uuid, &items);

        let loaded = from_nbt(&to_nbt(&[record]));
        let state = loaded[0].data.mob.as_ref().unwrap();
        assert!(state.chest);
        assert!(state.saddled);
        let pack = super::super::equine_chest::load(uuid, 15);
        assert_eq!(pack[4].name(), Some("minecraft:bread"));
        assert_eq!(pack[4].count, 3);
        assert!(pack[0].is_empty());
        super::super::equine_chest::clear(uuid);
    }

    #[test]
    fn a_cat_and_a_wolf_keep_their_variant_through_a_save() {
        let mut records = Vec::new();
        for (kind, name) in [
            (MobKind::Cat, "minecraft:calico"),
            (MobKind::Wolf, "minecraft:ashen"),
        ] {
            let mut record =
                ferrite_entity::mob::new_mob(kind, Vec3::new(0.5, 64.0, 0.5), &mut Rng::default());
            record.data.mob.as_mut().unwrap().variant = ferrite_entity::variant::index(kind, name);
            records.push(record);
        }
        let tag = to_nbt(&records);
        let Tag::Compound(root) = &tag else {
            unreachable!()
        };
        let Some(Tag::List(List::Compound(saved))) = root.get("Entities") else {
            unreachable!()
        };
        assert_eq!(
            saved[0].get("variant"),
            Some(&Tag::String("minecraft:calico".into()))
        );
        let loaded = from_nbt(&tag);
        let variants: Vec<_> = loaded
            .iter()
            .map(|r| {
                let state = r.data.mob.as_ref().unwrap();
                ferrite_entity::variant::name(state.kind, state.variant.unwrap())
            })
            .collect();
        assert_eq!(
            variants,
            [Some("minecraft:calico"), Some("minecraft:ashen")]
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
    fn new_projectile_kinds_keep_their_type_and_flight_age() {
        for kind in [
            arrow::Kind::ExperienceBottle,
            arrow::Kind::WindCharge,
            arrow::Kind::SplashPotion,
        ] {
            let mut record = arrow::new_thrown(
                kind,
                Vec3::new(0.5, 64.0, 0.5),
                Vec3::new(0.0, 0.0, 1.0),
                Some(Uuid::from_u128(7)),
            );
            record.data.arrow.as_mut().unwrap().age = 37;
            let loaded = from_nbt(&to_nbt(&[record]));
            assert_eq!(loaded.len(), 1, "{kind:?}");
            assert_eq!(arrow::kind_of(&loaded[0].data), Some(kind));
            let state = loaded[0].data.arrow.as_ref().unwrap();
            assert_eq!(state.age, 37);
            assert_eq!(state.owner, Some(Uuid::from_u128(7)));
        }
    }

    #[test]
    fn shoulder_parrots_survive_a_reconnect_save() {
        use ferrite_entity::shoulder::{ShoulderParrot, Shoulders};
        let dir = temp_world("shoulders");
        let uuid = Uuid::from_u128(0x5a1);
        let mut shoulders = Shoulders::default();
        let left = ShoulderParrot {
            uuid: Uuid::from_u128(0xaa),
            variant: 3,
            owner: Some(uuid),
        };
        let right = ShoulderParrot {
            uuid: Uuid::from_u128(0xbb),
            variant: 1,
            owner: Some(uuid),
        };
        shoulders.put(left);
        shoulders.put(right);
        save_shoulders(Some(&dir), uuid, &shoulders);
        assert!(shoulder_file(&dir, uuid).exists());
        assert_eq!(load_shoulders(Some(&dir), uuid), shoulders);

        // Releasing a parrot removes its side from the next login too.
        shoulders.release(ferrite_entity::shoulder::ShoulderSide::Left);
        save_shoulders(Some(&dir), uuid, &shoulders);
        let loaded = load_shoulders(Some(&dir), uuid);
        assert!(loaded.left.is_none());
        assert_eq!(loaded.right, Some(right));

        save_shoulders(Some(&dir), uuid, &Shoulders::default());
        assert!(!shoulder_file(&dir, uuid).exists());
        assert!(load_shoulders(Some(&dir), uuid).is_empty());
        let _ = std::fs::remove_dir_all(dir);
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

    #[test]
    fn custom_mob_equipment_and_parity_flags_survive_save_and_reload() {
        let mut zombie = mob::new_mob(
            MobKind::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        let helmet = ferrite_item::item_id("minecraft:iron_helmet").unwrap();
        let sword = ferrite_item::item_id("minecraft:iron_sword").unwrap();
        {
            let m = zombie.data.mob.as_mut().unwrap();
            m.equipment[5] = helmet;
            m.equipment[mob::MAIN_HAND as usize] = sword;
            m.in_water_time = 250;
        }
        let mut creeper = mob::new_mob(
            MobKind::Creeper,
            Vec3::new(2.5, 64.0, 0.5),
            &mut Rng::default(),
        );
        creeper.data.mob.as_mut().unwrap().ignited = true;

        let loaded = from_nbt(&to_nbt(&[zombie, creeper]));
        let lz = loaded[0].data.mob.as_ref().unwrap();
        assert_eq!(lz.equipment[5], helmet);
        assert_eq!(lz.equipment[mob::MAIN_HAND as usize], sword);
        assert_eq!(lz.in_water_time, 250);
        let lc = loaded[1].data.mob.as_ref().unwrap();
        assert!(lc.ignited);
        assert_eq!(
            loaded[1]
                .data
                .metadata
                .get(ferrite_entity::creeper::IGNITED_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::Boolean(true))
        );
    }

    #[test]
    fn mob_equipment_with_nbt_survives_disk_save_and_restart() {
        let dir = std::env::temp_dir().join(format!("ferrite-equip-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut store = ferrite_entity::EntityStore::default();
        let zombie = mob::new_mob(
            MobKind::Zombie,
            Vec3::new(3.5, 65.0, 4.5),
            &mut Rng::default(),
        );
        let uuid = zombie.data.uuid;
        let key = store.insert(zombie);

        let mut sword = ferrite_item::ItemStack::from_name("minecraft:diamond_sword").unwrap();
        sword.components.insert(
            ferrite_item::component_ids::CUSTOM_NAME,
            ferrite_item::Component::CustomName(ferrite_text::Component::text("Guard Blade")),
        );
        let helmet = ferrite_item::ItemStack::from_name("minecraft:golden_helmet").unwrap();
        assert!(mob::set_equipment_stack(
            &mut store,
            key,
            ferrite_entity::EquipmentSlot::MainHand,
            Some(sword.clone()),
        ));
        assert!(mob::set_equipment_stack(
            &mut store,
            key,
            ferrite_entity::EquipmentSlot::Head,
            Some(helmet.clone()),
        ));

        let records = vec![store.remove(key).unwrap()];
        save(&dir, (0, 0), &records).unwrap();

        // Simulate restart: load fresh from disk.
        let reloaded = load(&dir, (0, 0));
        let found = reloaded
            .iter()
            .find(|r| r.data.uuid == uuid)
            .expect("saved zombie reloaded");
        assert!(!found.data.equipment.is_dirty());
        assert_eq!(
            found
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::MainHand),
            Some(&sword)
        );
        assert_eq!(
            found
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Head),
            Some(&helmet)
        );
        let mob_state = found.data.mob.as_ref().unwrap();
        assert_eq!(mob_state.equipment[mob::MAIN_HAND as usize], sword.item);
        assert_eq!(mob_state.equipment[5], helmet.item);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drop_chances_body_saddle_and_reloaded_metadata_survive_save() {
        let dir = std::env::temp_dir().join(format!("ferrite-meta-equip-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut store = ferrite_entity::EntityStore::default();
        let horse = mob::new_mob(
            MobKind::Horse,
            Vec3::new(2.5, 65.0, 2.5),
            &mut Rng::default(),
        );
        let horse_uuid = horse.data.uuid;
        let horse_key = store.insert(horse);
        let armor = ferrite_item::ItemStack::from_name("minecraft:diamond_horse_armor").unwrap();
        let saddle = ferrite_item::ItemStack::from_name("minecraft:saddle").unwrap();
        assert!(mob::set_equipment_stack(
            &mut store,
            horse_key,
            ferrite_entity::EquipmentSlot::Body,
            Some(armor.clone()),
        ));
        assert!(mob::set_equipment_stack(
            &mut store,
            horse_key,
            ferrite_entity::EquipmentSlot::Saddle,
            Some(saddle.clone()),
        ));
        store
            .data_mut(horse_key)
            .unwrap()
            .equipment
            .set_drop_chance(ferrite_entity::EquipmentSlot::Body, 0.5);

        let mut zombie = mob::new_mob(
            MobKind::Zombie,
            Vec3::new(4.5, 65.0, 4.5),
            &mut Rng::default(),
        );
        zombie.data.mob.as_mut().unwrap().in_water_time =
            ferrite_entity::ai::DROWNED_CONVERSION_TICKS;
        let zombie_uuid = zombie.data.uuid;
        let zombie_key = store.insert(zombie);

        let mut dragon = mob::new_mob(
            MobKind::EnderDragon,
            Vec3::new(0.0, 80.0, 0.0),
            &mut Rng::default(),
        );
        dragon.data.mob.as_mut().unwrap().dragon_phase =
            ferrite_entity::dragon::DragonPhase::PerchFlaming;
        let dragon_uuid = dragon.data.uuid;
        let dragon_key = store.insert(dragon);

        let records = vec![
            store.remove(horse_key).unwrap(),
            store.remove(zombie_key).unwrap(),
            store.remove(dragon_key).unwrap(),
        ];
        save(&dir, (0, 0), &records).unwrap();

        let reloaded = load(&dir, (0, 0));
        let horse_reloaded = reloaded
            .iter()
            .find(|r| r.data.uuid == horse_uuid)
            .expect("reloaded horse");
        assert_eq!(
            horse_reloaded
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Body),
            Some(&armor)
        );
        assert_eq!(
            horse_reloaded
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Saddle),
            Some(&saddle)
        );
        assert_eq!(
            horse_reloaded
                .data
                .equipment
                .drop_chance(ferrite_entity::EquipmentSlot::Body),
            0.5
        );
        assert!(horse_reloaded.data.mob.as_ref().unwrap().saddled);
        assert_eq!(horse_reloaded.data.mob.as_ref().unwrap().armor, armor.item);

        let zombie_reloaded = reloaded
            .iter()
            .find(|r| r.data.uuid == zombie_uuid)
            .expect("reloaded zombie");
        assert_eq!(
            zombie_reloaded
                .data
                .metadata
                .get(ferrite_entity::mob::ZOMBIE_DROWNED_CONVERSION_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::Boolean(true))
        );

        let dragon_reloaded = reloaded
            .iter()
            .find(|r| r.data.uuid == dragon_uuid)
            .expect("reloaded dragon");
        assert_eq!(
            dragon_reloaded
                .data
                .metadata
                .get(ferrite_entity::dragon::PHASE_INDEX),
            Some(&ferrite_protocol::packets::play::MetaValue::Int(5))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sheared_snow_golem_and_a_player_built_iron_golem_keep_their_state() {
        let dir = std::env::temp_dir().join(format!("ferrite-golem-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = ferrite_entity::EntityStore::default();
        let mut rng = Rng::new(11);

        let mut snow = mob::new_mob(MobKind::SnowGolem, Vec3::new(1.5, 65.0, 1.5), &mut rng);
        snow.data.mob.as_mut().unwrap().sheared = true;
        ferrite_entity::mob::sync_state_metadata(&mut snow.data);
        let snow_uuid = snow.data.uuid;
        let snow_key = store.insert(snow);

        let mut iron = mob::new_mob(MobKind::IronGolem, Vec3::new(3.5, 65.0, 1.5), &mut rng);
        iron.data.mob.as_mut().unwrap().player_created = true;
        ferrite_entity::mob::sync_state_metadata(&mut iron.data);
        let iron_uuid = iron.data.uuid;
        let iron_key = store.insert(iron);

        let records = vec![
            store.remove(snow_key).unwrap(),
            store.remove(iron_key).unwrap(),
        ];
        save(&dir, (0, 0), &records).unwrap();
        let reloaded = load(&dir, (0, 0));
        use ferrite_protocol::packets::play::MetaValue;

        let snow = reloaded
            .iter()
            .find(|record| record.data.uuid == snow_uuid)
            .expect("reloaded snow golem");
        assert!(snow.data.mob.as_ref().unwrap().sheared);
        assert_eq!(
            snow.data
                .metadata
                .get(ferrite_entity::metadata::snow_golem::PUMPKIN),
            Some(&MetaValue::Byte(0))
        );

        let iron = reloaded
            .iter()
            .find(|record| record.data.uuid == iron_uuid)
            .expect("reloaded iron golem");
        assert!(iron.data.mob.as_ref().unwrap().player_created);
        assert_eq!(
            iron.data
                .metadata
                .get(ferrite_entity::metadata::iron_golem::FLAGS),
            Some(&MetaValue::Byte(
                ferrite_entity::metadata::iron_golem::FLAG_PLAYER_CREATED
            ))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_mob_metadata_wolf_body_pig_saddle_and_passenger_equipment_survive_save() {
        let _turn = super::super::ride::test_turn();
        let dir = std::env::temp_dir().join(format!("ferrite-breadth-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut store = ferrite_entity::EntityStore::default();
        let mut rng = Rng::new(7);

        let mut golem = mob::new_mob(MobKind::SnowGolem, Vec3::new(1.5, 65.0, 1.5), &mut rng);
        golem.data.mob.as_mut().unwrap().sheared = true;
        let golem_uuid = golem.data.uuid;
        let golem_key = store.insert(golem);

        let mut bogged = mob::new_mob(MobKind::Bogged, Vec3::new(2.5, 65.0, 1.5), &mut rng);
        bogged.data.mob.as_mut().unwrap().sheared = true;
        let bogged_uuid = bogged.data.uuid;
        let bogged_key = store.insert(bogged);

        let mut frog = mob::new_mob(MobKind::Frog, Vec3::new(3.5, 65.0, 1.5), &mut rng);
        frog.data.mob.as_mut().unwrap().variant = Some(2);
        let frog_uuid = frog.data.uuid;
        let frog_key = store.insert(frog);

        let mut fox = mob::new_mob(MobKind::Fox, Vec3::new(4.5, 65.0, 1.5), &mut rng);
        fox.data.mob.as_mut().unwrap().variant = Some(1);
        let fox_uuid = fox.data.uuid;
        let fox_key = store.insert(fox);

        let mut mooshroom = mob::new_mob(MobKind::Mooshroom, Vec3::new(5.5, 65.0, 1.5), &mut rng);
        mooshroom.data.mob.as_mut().unwrap().variant = Some(1);
        let mooshroom_uuid = mooshroom.data.uuid;
        let mooshroom_key = store.insert(mooshroom);

        let mut rabbit = mob::new_mob(MobKind::Rabbit, Vec3::new(6.5, 65.0, 1.5), &mut rng);
        rabbit.data.mob.as_mut().unwrap().variant = Some(4);
        let rabbit_uuid = rabbit.data.uuid;
        let rabbit_key = store.insert(rabbit);

        let wolf = mob::new_mob(MobKind::Wolf, Vec3::new(7.5, 65.0, 1.5), &mut rng);
        let wolf_uuid = wolf.data.uuid;
        let wolf_key = store.insert(wolf);
        let wolf_armor = ferrite_item::ItemStack::from_name("minecraft:wolf_armor").unwrap();
        assert!(mob::set_equipment_stack(
            &mut store,
            wolf_key,
            ferrite_entity::EquipmentSlot::Body,
            Some(wolf_armor.clone()),
        ));
        assert!(mob::set_guaranteed_drop(
            &mut store,
            wolf_key,
            ferrite_entity::EquipmentSlot::Body,
        ));

        let pig = mob::new_mob(MobKind::Pig, Vec3::new(8.5, 65.0, 1.5), &mut rng);
        let pig_uuid = pig.data.uuid;
        let pig_key = store.insert(pig);
        assert!(ferrite_entity::pig::set_saddled(&mut store, pig_key, true));

        // Spider jockey: passenger relationship + rider equipment + custom drop chance.
        let spider = mob::new_mob(MobKind::Spider, Vec3::new(9.5, 65.0, 1.5), &mut rng);
        let spider_uuid = spider.data.uuid;
        let spider_key = store.insert(spider);
        let skeleton = mob::new_mob(MobKind::Skeleton, Vec3::new(9.5, 65.5, 1.5), &mut rng);
        let skeleton_uuid = skeleton.data.uuid;
        let skeleton_key = store.insert(skeleton);
        let helm = ferrite_item::ItemStack::from_name("minecraft:chainmail_helmet").unwrap();
        assert!(mob::set_equipment_stack(
            &mut store,
            skeleton_key,
            ferrite_entity::EquipmentSlot::Head,
            Some(helm.clone()),
        ));
        assert!(mob::set_drop_chance(
            &mut store,
            skeleton_key,
            ferrite_entity::EquipmentSlot::Head,
            0.35,
        ));
        super::super::ride::restore(skeleton_uuid, spider_uuid);

        let records = vec![
            store.remove(golem_key).unwrap(),
            store.remove(bogged_key).unwrap(),
            store.remove(frog_key).unwrap(),
            store.remove(fox_key).unwrap(),
            store.remove(mooshroom_key).unwrap(),
            store.remove(rabbit_key).unwrap(),
            store.remove(wolf_key).unwrap(),
            store.remove(pig_key).unwrap(),
            store.remove(spider_key).unwrap(),
            store.remove(skeleton_key).unwrap(),
        ];
        save(&dir, (0, 0), &records).unwrap();
        super::super::ride::forget(skeleton_uuid);
        assert_eq!(super::super::ride::vehicle_of(skeleton_uuid), None);

        let reloaded = load(&dir, (0, 0));
        let find = |u| reloaded.iter().find(|r| r.data.uuid == u).unwrap();
        use ferrite_protocol::packets::play::MetaValue;

        assert_eq!(
            find(golem_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::snow_golem::PUMPKIN),
            Some(&MetaValue::Byte(0))
        );
        assert_eq!(
            find(bogged_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::bogged::SHEARED),
            Some(&MetaValue::Boolean(true))
        );
        assert_eq!(
            find(frog_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::frog::VARIANT),
            Some(&MetaValue::FrogVariant(2))
        );
        assert_eq!(
            find(fox_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::fox::TYPE),
            Some(&MetaValue::Int(1))
        );
        assert_eq!(
            find(mooshroom_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::mooshroom::TYPE),
            Some(&MetaValue::Int(1))
        );
        assert_eq!(
            find(rabbit_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::rabbit::TYPE),
            Some(&MetaValue::Int(4))
        );
        let wolf_r = find(wolf_uuid);
        assert_eq!(
            wolf_r
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Body),
            Some(&wolf_armor)
        );
        assert!(
            wolf_r
                .data
                .equipment
                .is_preserved(ferrite_entity::EquipmentSlot::Body)
        );

        let pig_r = find(pig_uuid);
        assert!(pig_r.data.mob.as_ref().unwrap().saddled);
        assert_eq!(
            pig_r
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Saddle)
                .and_then(|s| s.name()),
            Some("minecraft:saddle")
        );
        assert!(
            pig_r
                .data
                .equipment
                .is_preserved(ferrite_entity::EquipmentSlot::Saddle)
        );

        // Passenger relationship + rider equipment + drop chance restored.
        assert_eq!(
            super::super::ride::vehicle_of(skeleton_uuid),
            Some(spider_uuid)
        );
        let skel_r = find(skeleton_uuid);
        assert_eq!(
            skel_r
                .data
                .equipment
                .get(ferrite_entity::EquipmentSlot::Head),
            Some(&helm)
        );
        assert_eq!(
            skel_r
                .data
                .equipment
                .drop_chance(ferrite_entity::EquipmentSlot::Head),
            0.35
        );

        super::super::ride::forget(skeleton_uuid);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn piglin_inventory_and_admiring_state_survive_nbt_and_disk_reload() {
        let mut store = ferrite_entity::EntityStore::default();
        let mut rng = ferrite_entity::Rng::new(0x5A7E);
        let key = store.insert(mob::new_mob(
            MobKind::Piglin,
            Vec3::new(3.5, 64.0, 4.5),
            &mut rng,
        ));
        mob::state_mut(&mut store, key).unwrap().age = 0;

        let pork = ferrite_item::ItemStack::from_name("minecraft:porkchop")
            .unwrap()
            .copy_with_count(3);
        let gold_nugget = ferrite_item::ItemStack::from_name("minecraft:gold_nugget")
            .unwrap()
            .copy_with_count(7);
        {
            let inv = ferrite_entity::piglin::inventory_mut(&mut store, key).unwrap();
            inv.set(0, pork.clone());
            inv.set(5, gold_nugget.clone());
        }
        assert!(ferrite_entity::piglin::start_admiring(&mut store, key).is_some());
        mob::state_mut(&mut store, key).unwrap().admiring = 42;

        let record = store.remove(key).unwrap();
        let uuid = record.data.uuid;
        let dir = std::env::temp_dir().join(format!("ferrite_piglin_save_{uuid}"));
        let _ = std::fs::remove_dir_all(&dir);
        save(&dir, (0, 0), &[record]).unwrap();
        let loaded = load(&dir, (0, 0));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(loaded.len(), 1);
        let mut loaded_store = ferrite_entity::EntityStore::default();
        let loaded_key = loaded_store.insert(loaded.into_iter().next().unwrap());
        let state = mob::state(&loaded_store, loaded_key).unwrap();
        assert_eq!(state.admiring, 42);
        assert_eq!(
            mob::equipment_stack(
                &loaded_store,
                loaded_key,
                ferrite_entity::EquipmentSlot::OffHand
            )
            .as_ref()
            .and_then(|s| s.name()),
            Some("minecraft:gold_ingot")
        );
        assert_eq!(
            loaded_store
                .data(loaded_key)
                .unwrap()
                .equipment
                .drop_chance(ferrite_entity::EquipmentSlot::OffHand),
            ferrite_entity::equipment::GUARANTEED_DROP_CHANCE
        );
        let inv = ferrite_entity::piglin::inventory(&loaded_store, loaded_key).unwrap();
        assert_eq!(inv.slots().len(), ferrite_entity::piglin::INVENTORY_SLOTS);
        assert_eq!(inv.get(0), Some(&pork));
        assert_eq!(inv.get(1), Some(&gold_nugget));
    }

    #[test]
    fn turtle_home_and_has_egg_round_trip_and_malformed_home_falls_back() {
        let mut record = mob::new_mob(
            MobKind::Turtle,
            Vec3::new(4.5, 63.0, -2.5),
            &mut Rng::new(1),
        );
        {
            let state = record.data.mob.as_mut().unwrap();
            state.home = Some([7, 64, -3]);
            state.has_egg = true;
        }
        let mut loaded = from_nbt(&to_nbt(&[record]));
        let turtle = loaded.pop().unwrap();
        let state = turtle.data.mob.as_ref().unwrap();
        assert_eq!(state.home, Some([7, 64, -3]));
        assert!(state.has_egg);
        assert!(!state.laying_egg);

        // `home_pos` with two entries is malformed; vanilla's reader falls back to the position
        // it loaded the entity at instead of panicking on the int array.
        let record = mob::new_mob(
            MobKind::Turtle,
            Vec3::new(-3.5, 70.0, 8.5),
            &mut Rng::new(2),
        );
        let mut nbt = to_nbt(&[record]);
        let Tag::Compound(root) = &mut nbt else {
            panic!("expected root compound");
        };
        let Tag::List(List::Compound(entities)) = root.get_mut("Entities").unwrap() else {
            panic!("expected Entities list");
        };
        entities[0].insert("home_pos".into(), Tag::IntArray(vec![1, 2]));
        let loaded = from_nbt(&nbt);
        let pos = loaded[0].pos;
        assert_eq!(
            loaded[0].data.mob.as_ref().unwrap().home,
            Some([
                pos.x.floor() as i32,
                pos.y.floor() as i32,
                pos.z.floor() as i32
            ])
        );
    }

    #[test]
    fn per_mob_flags_and_sound_variants_survive_nbt_and_disk_reload() {
        use ferrite_protocol::packets::play::MetaValue;
        let dir = temp_world("mob-flags");
        let mut rng = Rng::new(0xF1A6);
        let mut store = ferrite_entity::EntityStore::default();

        let mut goat = mob::new_mob(MobKind::Goat, Vec3::new(1.5, 64.0, 1.5), &mut rng);
        {
            let state = goat.data.mob.as_mut().unwrap();
            state.screaming = true;
            state.has_left_horn = false;
            state.has_right_horn = true;
        }
        let goat_uuid = goat.data.uuid;
        let goat_key = store.insert(goat);

        let first_player = Uuid::from_u128(0xA11CE);
        let second_player = Uuid::from_u128(0xB0B);
        let mut fox = mob::new_mob(MobKind::Fox, Vec3::new(2.5, 64.0, 1.5), &mut rng);
        {
            let state = fox.data.mob.as_mut().unwrap();
            state.variant = Some(1);
            state.sitting = true;
            state.fox_crouching = true;
            state.fox_sleeping = true;
            state.trusted = [Some(first_player), Some(second_player)];
        }
        let fox_uuid = fox.data.uuid;
        let fox_key = store.insert(fox);

        let mut golem = mob::new_mob(MobKind::IronGolem, Vec3::new(3.5, 64.0, 1.5), &mut rng);
        golem.data.mob.as_mut().unwrap().player_created = true;
        let golem_uuid = golem.data.uuid;
        let golem_key = store.insert(golem);

        let mut cow = mob::new_mob(MobKind::Cow, Vec3::new(4.5, 64.0, 1.5), &mut rng);
        cow.data.mob.as_mut().unwrap().sound_variant = Some(1);
        let cow_uuid = cow.data.uuid;
        let cow_key = store.insert(cow);

        let mut wolf = mob::new_mob(MobKind::Wolf, Vec3::new(5.5, 64.0, 1.5), &mut rng);
        wolf.data.mob.as_mut().unwrap().sound_variant = Some(6);
        let wolf_uuid = wolf.data.uuid;
        let wolf_key = store.insert(wolf);

        let records: Vec<_> = [goat_key, fox_key, golem_key, cow_key, wolf_key]
            .into_iter()
            .filter_map(|key| store.remove(key))
            .collect();
        save(&dir, (0, 0), &records).unwrap();
        let reloaded = load(&dir, (0, 0));
        let _ = std::fs::remove_dir_all(&dir);

        let find = |uuid| reloaded.iter().find(|r| r.data.uuid == uuid).unwrap();

        let goat = find(goat_uuid);
        let state = goat.data.mob.as_ref().unwrap();
        assert!(state.screaming);
        assert!(!state.has_left_horn);
        assert!(state.has_right_horn);
        assert_eq!(
            goat.data
                .metadata
                .get(ferrite_entity::metadata::goat::IS_SCREAMING),
            Some(&MetaValue::Boolean(true))
        );
        assert_eq!(
            goat.data
                .metadata
                .get(ferrite_entity::metadata::goat::HAS_LEFT_HORN),
            Some(&MetaValue::Boolean(false))
        );

        let fox = find(fox_uuid);
        let state = fox.data.mob.as_ref().unwrap();
        assert_eq!(state.trusted, [Some(first_player), Some(second_player)]);
        assert!(state.sitting && state.fox_crouching && state.fox_sleeping);
        assert_eq!(
            fox.data.metadata.get(ferrite_entity::metadata::fox::FLAGS),
            Some(&MetaValue::Byte(
                (ferrite_entity::metadata::fox::FLAG_SITTING
                    | ferrite_entity::metadata::fox::FLAG_CROUCHING
                    | ferrite_entity::metadata::fox::FLAG_SLEEPING) as i8
            ))
        );
        assert_eq!(
            fox.data
                .metadata
                .get(ferrite_entity::metadata::fox::TRUSTED_0),
            Some(&MetaValue::OptionalUuid(Some(first_player)))
        );
        assert_eq!(
            fox.data
                .metadata
                .get(ferrite_entity::metadata::fox::TRUSTED_1),
            Some(&MetaValue::OptionalUuid(Some(second_player)))
        );

        let golem = find(golem_uuid);
        assert!(golem.data.mob.as_ref().unwrap().player_created);
        assert_eq!(
            golem
                .data
                .metadata
                .get(ferrite_entity::metadata::iron_golem::FLAGS),
            Some(&MetaValue::Byte(
                ferrite_entity::metadata::iron_golem::FLAG_PLAYER_CREATED
            ))
        );

        assert_eq!(
            find(cow_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::animal_variant::COW_SOUND_VARIANT),
            Some(&MetaValue::CowSoundVariant(1))
        );
        assert_eq!(
            find(wolf_uuid)
                .data
                .metadata
                .get(ferrite_entity::metadata::animal_variant::WOLF_SOUND_VARIANT),
            Some(&MetaValue::WolfSoundVariant(6))
        );
    }

    #[test]
    fn a_bee_keeps_its_hive_nectar_and_crop_state_across_a_save() {
        let mut rng = Rng::new(8);
        let mut record = mob::new_mob(MobKind::Bee, Vec3::new(2.5, 64.0, 3.5), &mut rng);
        {
            let bee = record.data.mob.as_mut().unwrap();
            bee.has_nectar = true;
            bee.has_stung = true;
            bee.hive_pos = Some([4, 70, -2]);
            bee.flower_pos = Some([5, 65, 3]);
            bee.crops_grown = 7;
            bee.ticks_since_pollination = 120;
            bee.stay_out_of_hive = 33;
            bee.sting_ticks = 200;
            bee.ticks_without_nectar = 9;
        }

        let loaded = from_nbt(&to_nbt(&[record]));
        assert_eq!(loaded.len(), 1);
        let bee = loaded[0].data.mob.as_ref().unwrap();
        assert!(bee.has_nectar);
        assert!(bee.has_stung);
        assert_eq!(bee.hive_pos, Some([4, 70, -2]));
        assert_eq!(bee.flower_pos, Some([5, 65, 3]));
        assert_eq!(bee.crops_grown, 7);
        assert_eq!(bee.ticks_since_pollination, 120);
        assert_eq!(bee.stay_out_of_hive, 33);
        assert_eq!(bee.sting_ticks, 200);
        assert_eq!(bee.ticks_without_nectar, 9);
    }
}
