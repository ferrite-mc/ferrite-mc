//! Villagers: professions, workstations, trade offers, demand pricing and restocking.
//!
//! The offer pools come from the 26.3 data generator (`ferrite_data::villager_trades`), so a
//! farmer's level-1 pool is the real wheat/potato/carrot/beetroot/bread list. Each villager
//! draws its offers deterministically from its UUID, so the same villager always offers the
//! same trades.
//!
//! The villager follows the Java daily schedule: it claims a workstation and a bed in its
//! region, walks to the workstation during work, meets near a bell and sleeps at night. A
//! restock needs a valid workstation, the work activity and a sold-out offer, and is limited to
//! two per day. Villagers never level up from working; they level up from completed trades
//! (vanilla's trade XP thresholds).
//!
//! Simplifications, documented rather than hidden:
//! - Workstation/bed claims are local to one region. There is no global point-of-interest table,
//!   so two regions can claim the same block near a boundary.
//! - The pathfinder keeps 16 waypoints; a destination farther than that needs a shorter path or
//!   the direct fallback, and can still be blocked by terrain.
//! - Sleeping pose, profession-specific work goals (harvesting, fishing, bookshelf inspection),
//!   breeding and gossip decay are not modelled.
//! - Offers with a `merchant_predicate` are skipped, and results whose data attaches item
//!   modifiers keep their base item (see `ferrite_data::villager_trades`).
//! - The villager type is always plains.
//! - Gossip is only gained by trading. Attacking or killing villagers, curing them, gossip
//!   sharing between villagers and gossip decay are not modelled.

use crate::EntityKey;
use crate::mob::MobKind;
use crate::physics::BlockGetter;
use crate::store::EntityStore;
use ferrite_inventory::Trade;
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Vanilla `Villager.DATA_VILLAGER_DATA` accessor index in 26.3 (from the class-tree id
/// registry: `Entity` 0..7, `LivingEntity` 8..14, `Mob` 15, `AgeableMob` 16..17,
/// `AbstractVillager` 18, `Villager` 19..20).
pub const VILLAGER_DATA_INDEX: u8 = 19;
/// Vanilla `Villager.DATA_VILLAGER_DATA_FINALIZED`: the client waits for this before showing the
/// villager's profession.
pub const VILLAGER_DATA_FINALIZED_INDEX: u8 = 20;
/// Serializer id of `EntityDataSerializers.VILLAGER_DATA` in 26.3.
pub const VILLAGER_DATA_SERIALIZER: i32 = 18;
/// Blocks searched around a villager for a workstation.
pub const WORKSTATION_RADIUS: i32 = 16;
/// Vertical reach of the workstation search.
pub const WORKSTATION_VERTICAL: i32 = 4;
/// Ticks between workstation searches while a villager is unemployed.
pub const WORKSTATION_SEARCH_TICKS: u16 = 40;
/// Vanilla minimum time between two restocks (`Villager.shouldRestock`, 2400 ticks).
pub const RESTOCK_COOLDOWN_TICKS: i64 = 2400;
/// `RestocksToday` is forgotten after this long without a restock (12000 ticks).
pub const RESTOCK_FORGET_TICKS: i64 = 12000;
/// Vanilla allows two restocks per workday.
pub const RESTOCKS_PER_DAY: u8 = 2;
/// Horizontal search radius for a bed.
pub const BED_RADIUS: i32 = 32;
/// Vertical search radius for a bed.
pub const BED_VERTICAL: i32 = 4;
/// Horizontal search radius for the village bell used during a meeting.
pub const BELL_RADIUS: i32 = 32;
/// Vertical search radius for the village bell.
pub const BELL_VERTICAL: i32 = 4;
/// Vanilla job-site reach: eight blocks horizontally and at most nine blocks below the feet.
pub const WORK_REACH_HORIZONTAL: f64 = 8.0;
/// How far below the villager's feet a workstation may be for a restock.
pub const WORK_REACH_BELOW: f64 = 9.0;
/// How far a villager wanders from where it spawned before walking back.
pub const WANDER_RADIUS: f64 = 16.0;
/// Highest trade level.
pub const MAX_LEVEL: u8 = 5;
/// Vanilla `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`: XP needed to reach levels 2, 3, 4 and 5.
pub const NEXT_LEVEL_XP: [i32; 4] = [10, 70, 150, 250];

/// The `minecraft:villager_profession` entries this server models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profession {
    #[default]
    None,
    Armorer,
    Butcher,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    Leatherworker,
    Librarian,
    Mason,
    Nitwit,
    Shepherd,
    Toolsmith,
    Weaponsmith,
}

impl Profession {
    /// Namespaced registry name, e.g. `"minecraft:farmer"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "minecraft:none",
            Self::Armorer => "minecraft:armorer",
            Self::Butcher => "minecraft:butcher",
            Self::Cartographer => "minecraft:cartographer",
            Self::Cleric => "minecraft:cleric",
            Self::Farmer => "minecraft:farmer",
            Self::Fisherman => "minecraft:fisherman",
            Self::Fletcher => "minecraft:fletcher",
            Self::Leatherworker => "minecraft:leatherworker",
            Self::Librarian => "minecraft:librarian",
            Self::Mason => "minecraft:mason",
            Self::Nitwit => "minecraft:nitwit",
            Self::Shepherd => "minecraft:shepherd",
            Self::Toolsmith => "minecraft:toolsmith",
            Self::Weaponsmith => "minecraft:weaponsmith",
        }
    }

    /// The profession with this registry name (`minecraft:` optional).
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        Some(match name {
            "none" => Self::None,
            "armorer" => Self::Armorer,
            "butcher" => Self::Butcher,
            "cartographer" => Self::Cartographer,
            "cleric" => Self::Cleric,
            "farmer" => Self::Farmer,
            "fisherman" => Self::Fisherman,
            "fletcher" => Self::Fletcher,
            "leatherworker" => Self::Leatherworker,
            "librarian" => Self::Librarian,
            "mason" => Self::Mason,
            "nitwit" => Self::Nitwit,
            "shepherd" => Self::Shepherd,
            "toolsmith" => Self::Toolsmith,
            "weaponsmith" => Self::Weaponsmith,
            _ => return None,
        })
    }

    /// Protocol id in `minecraft:villager_profession`.
    pub fn id(self) -> i32 {
        ferrite_data::registries::id("minecraft:villager_profession", self.name()).unwrap_or(0)
    }

    /// The profession a workstation block assigns, or `None` when the block is not a
    /// workstation. Names may omit the `minecraft:` prefix.
    pub fn from_workstation(block: &str) -> Option<Self> {
        let block = block.strip_prefix("minecraft:").unwrap_or(block);
        Some(match block {
            "lectern" => Self::Librarian,
            "composter" => Self::Farmer,
            "blast_furnace" => Self::Armorer,
            "smithing_table" => Self::Weaponsmith,
            "cartography_table" => Self::Cartographer,
            "fletching_table" => Self::Fletcher,
            "grindstone" => Self::Toolsmith,
            "brewing_stand" => Self::Cleric,
            "barrel" => Self::Fisherman,
            "cauldron" => Self::Leatherworker,
            "loom" => Self::Shepherd,
            "stonecutter" => Self::Mason,
            "smoker" => Self::Butcher,
            _ => return None,
        })
    }

    /// The workstation block this profession is known by, if any.
    pub const fn workstation(self) -> Option<&'static str> {
        Some(match self {
            Self::Librarian => "minecraft:lectern",
            Self::Farmer => "minecraft:composter",
            Self::Armorer => "minecraft:blast_furnace",
            Self::Weaponsmith => "minecraft:smithing_table",
            Self::Cartographer => "minecraft:cartography_table",
            Self::Fletcher => "minecraft:fletching_table",
            Self::Toolsmith => "minecraft:grindstone",
            Self::Cleric => "minecraft:brewing_stand",
            Self::Fisherman => "minecraft:barrel",
            Self::Leatherworker => "minecraft:cauldron",
            Self::Shepherd => "minecraft:loom",
            Self::Mason => "minecraft:stonecutter",
            Self::Butcher => "minecraft:smoker",
            Self::None | Self::Nitwit => return None,
        })
    }
}

/// The `minecraft:villager_type` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VillagerType {
    Desert,
    Jungle,
    #[default]
    Plains,
    Savanna,
    Snow,
    Swamp,
    Taiga,
}

impl VillagerType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Desert => "minecraft:desert",
            Self::Jungle => "minecraft:jungle",
            Self::Plains => "minecraft:plains",
            Self::Savanna => "minecraft:savanna",
            Self::Snow => "minecraft:snow",
            Self::Swamp => "minecraft:swamp",
            Self::Taiga => "minecraft:taiga",
        }
    }

    pub fn id(self) -> i32 {
        ferrite_data::registries::id("minecraft:villager_type", self.name()).unwrap_or(0)
    }

    /// The villager type with this registry name (`minecraft:` optional).
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        Some(match name {
            "desert" => Self::Desert,
            "jungle" => Self::Jungle,
            "plains" => Self::Plains,
            "savanna" => Self::Savanna,
            "snow" => Self::Snow,
            "swamp" => Self::Swamp,
            "taiga" => Self::Taiga,
            _ => return None,
        })
    }
}

/// The activity a villager follows from its daily schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    /// Wander around home, or stand near a missing destination.
    #[default]
    Idle,
    /// A baby's daytime play period.
    Play,
    /// Walk to and stand at the claimed workstation.
    Work,
    /// Walk to the village bell (or home when there is none).
    Meet,
    /// Walk to the claimed bed, or home when there is no bed.
    Rest,
}

/// The Java schedule for `day_time` (0 is 06:00).
pub const fn activity_at(day_time: i64, baby: bool, employed: bool) -> Activity {
    let tick = day_time.rem_euclid(24_000);
    if tick < 10 {
        Activity::Idle
    } else if tick >= 12_000 {
        Activity::Rest
    } else if baby {
        if (tick >= 3000 && tick < 6000) || (tick >= 10_000 && tick < 12_000) {
            Activity::Play
        } else {
            Activity::Idle
        }
    } else if tick >= 11_000 {
        Activity::Idle
    } else if tick >= 9000 {
        Activity::Meet
    } else if tick >= 2000 && employed {
        Activity::Work
    } else {
        Activity::Idle
    }
}

/// Vanilla `GossipType`: the five kinds of memory a villager keeps about a player.
///
/// The weight turns stored gossip points into reputation and the cap bounds the stored value,
/// both from vanilla's 26.3 `GossipType` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GossipType {
    MajorNegative,
    MinorNegative,
    MinorPositive,
    MajorPositive,
    Trading,
}

impl GossipType {
    /// The NBT id, e.g. `"minor_positive"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::MajorNegative => "major_negative",
            Self::MinorNegative => "minor_negative",
            Self::MinorPositive => "minor_positive",
            Self::MajorPositive => "major_positive",
            Self::Trading => "trading",
        }
    }

    /// Parses [`Self::name`], with or without a `minecraft:` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        Some(match name {
            "major_negative" => Self::MajorNegative,
            "minor_negative" => Self::MinorNegative,
            "minor_positive" => Self::MinorPositive,
            "major_positive" => Self::MajorPositive,
            "trading" => Self::Trading,
            _ => return None,
        })
    }

    /// Reputation per stored gossip point (vanilla `GossipType.weight`).
    pub const fn weight(self) -> i32 {
        match self {
            Self::MajorNegative => -5,
            Self::MinorNegative => -1,
            Self::MinorPositive | Self::Trading => 1,
            Self::MajorPositive => 5,
        }
    }

    /// Cap on the stored gossip value (vanilla `GossipType.max`).
    pub const fn max(self) -> i32 {
        match self {
            Self::MajorNegative => 100,
            Self::MinorNegative => 200,
            Self::MinorPositive | Self::Trading => 25,
            Self::MajorPositive => 20,
        }
    }
}

/// Gossip points a completed sale adds (vanilla gives two trading points per trade).
pub const TRADING_GOSSIP: i32 = 2;

/// The gossip a villager remembers about players, keyed by player UUID (vanilla `Gossips`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gossips {
    entries: BTreeMap<Uuid, BTreeMap<GossipType, i32>>,
}

impl Gossips {
    /// Adds gossip points, capped at the type's maximum (vanilla `Gossips.add`).
    pub fn add(&mut self, player: Uuid, kind: GossipType, points: i32) {
        if points > 0 {
            self.set(player, kind, self.get(player, kind).saturating_add(points));
        }
    }

    /// Sets one type's stored value, clamped to `0..=kind.max()`. Used when loading a save.
    pub fn set(&mut self, player: Uuid, kind: GossipType, value: i32) {
        let value = value.clamp(0, kind.max());
        if value == 0 {
            if let Some(values) = self.entries.get_mut(&player) {
                values.remove(&kind);
                if values.is_empty() {
                    self.entries.remove(&player);
                }
            }
        } else {
            self.entries.entry(player).or_default().insert(kind, value);
        }
    }

    /// The stored value of one gossip type for a player.
    pub fn get(&self, player: Uuid, kind: GossipType) -> i32 {
        self.entries
            .get(&player)
            .and_then(|values| values.get(&kind))
            .copied()
            .unwrap_or(0)
    }

    /// A player's reputation with this villager: every stored value times its type's weight
    /// (vanilla `Gossips.getReputation`).
    pub fn reputation(&self, player: Uuid) -> i32 {
        self.entries
            .get(&player)
            .map(|values| {
                values
                    .iter()
                    .map(|(kind, value)| kind.weight() * value)
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Every stored entry, in a stable order for saving.
    pub fn entries(&self) -> impl Iterator<Item = (Uuid, GossipType, i32)> + '_ {
        self.entries.iter().flat_map(|(player, values)| {
            values
                .iter()
                .map(move |(kind, value)| (*player, *kind, *value))
        })
    }
}

/// A villager's living trade state; travels with the entity across region hand-offs.
#[derive(Debug, Clone)]
pub struct VillagerState {
    pub profession: Profession,
    pub villager_type: VillagerType,
    /// Trade level, 1..=5.
    pub level: u8,
    /// Vanilla `villagerXp`.
    pub xp: i32,
    /// The workstation block that gave the profession, kept as the villager's point of
    /// interest.
    pub workstation: Option<[i32; 3]>,
    /// The foot of the bed this villager claimed, if any.
    pub bed: Option<[i32; 3]>,
    /// The villager's offers, with their uses and demand.
    pub offers: Vec<Trade>,
    /// What this villager remembers about each player (vanilla `Gossips`).
    pub gossips: Gossips,
    /// Game time of the last restock (vanilla `LastRestock`).
    pub last_restock: i64,
    /// Restocks completed today (vanilla `RestocksToday`).
    pub restocks_today: u8,
    /// Day index whose restocks have already been counted.
    pub restock_day: i64,
    /// The schedule activity for the current tick; derived and not persisted.
    pub activity: Activity,
    /// Where the AI should walk this tick; derived and not persisted.
    pub destination: Option<[i32; 3]>,
    /// Bell (or home fallback) chosen for the current meeting; derived and not persisted.
    pub meeting_point: Option<[i32; 3]>,
    /// Ticks until the next workstation or bed search.
    pub search_timer: u16,
    /// Where the villager spawned; it wanders back within [`WANDER_RADIUS`].
    pub home: [f64; 3],
}

impl VillagerState {
    /// An unemployed villager standing at `home`.
    pub fn new(villager_type: VillagerType, home: [f64; 3]) -> Self {
        Self {
            profession: Profession::None,
            villager_type,
            level: 1,
            xp: 0,
            workstation: None,
            bed: None,
            offers: Vec::new(),
            gossips: Gossips::default(),
            last_restock: -RESTOCK_COOLDOWN_TICKS,
            restocks_today: 0,
            restock_day: -1,
            activity: Activity::Idle,
            destination: None,
            meeting_point: None,
            search_timer: 0,
            home,
        }
    }

    /// The offers a villager of `profession` and `level` draws, levels 1..=level.
    pub fn generate_offers(&self, uuid: Uuid) -> Vec<Trade> {
        offers_for(self.profession, self.level, uuid)
    }

    /// The player's reputation with this villager (vanilla `Villager.getPlayerReputation`).
    pub fn reputation(&self, player: Uuid) -> i32 {
        self.gossips.reputation(player)
    }

    /// The offers as this player sees them, with the reputation adjustment applied to a copy
    /// (vanilla `AbstractVillager.updateSpecialPrices`). The stored offers keep their base
    /// special price, so another player's screen is unaffected.
    pub fn offers_for_player(&self, player: Uuid) -> Vec<Trade> {
        let reputation = self.gossips.reputation(player);
        let mut offers = self.offers.clone();
        if reputation != 0 {
            for offer in &mut offers {
                offer.special_price +=
                    reputation_price_adjustment(reputation, offer.price_multiplier);
            }
        }
        offers
    }

    /// Merges the sales made through one player's menu snapshot into the authoritative offers.
    ///
    /// Only uses above the current value are applied, so a stale second screen cannot rewind a
    /// sale or push an offer past `max_uses`; demand rises with each applied sale. Returns the
    /// number of sales applied.
    pub fn merge_trades(&mut self, trades: &[Trade]) -> i32 {
        if self.offers.len() != trades.len() {
            return 0;
        }
        let mut sold = 0;
        for (offer, trade) in self.offers.iter_mut().zip(trades) {
            let available = (offer.max_uses - offer.uses).max(0);
            let used = (trade.uses - offer.uses).clamp(0, available);
            if used == 0 {
                continue;
            }
            offer.uses += used;
            offer.demand = offer.demand.saturating_add(used);
            sold += used;
        }
        sold
    }

    /// Records one completed sale for `player` (vanilla's trading gossip).
    pub fn record_trade(&mut self, player: Uuid) {
        self.gossips
            .add(player, GossipType::Trading, TRADING_GOSSIP);
    }
}

/// Vanilla `AbstractVillager.updateSpecialPrices`: a positive reputation lowers the price by
/// `floor(reputation * priceMultiplier)`, a negative one raises it by the same amount.
pub fn reputation_price_adjustment(reputation: i32, price_multiplier: f32) -> i32 {
    -((reputation as f32 * price_multiplier).floor() as i32)
}

/// Defines the villager's synched data values and writes the current state into them.
pub fn define_metadata(metadata: &mut crate::Metadata, state: &VillagerState) {
    metadata.define(
        VILLAGER_DATA_INDEX,
        MetaValue::VillagerData {
            kind: state.villager_type.id(),
            profession: state.profession.id(),
            level: i32::from(state.level),
        },
    );
    metadata.define(VILLAGER_DATA_FINALIZED_INDEX, MetaValue::Boolean(false));
    sync_metadata(metadata, state);
}

/// Updates the synched villager data (index 19) and marks the data finalized (index 20).
pub fn sync_metadata(metadata: &mut crate::Metadata, state: &VillagerState) {
    metadata.set(
        VILLAGER_DATA_INDEX,
        MetaValue::VillagerData {
            kind: state.villager_type.id(),
            profession: state.profession.id(),
            level: i32::from(state.level),
        },
    );
    metadata.set(VILLAGER_DATA_FINALIZED_INDEX, MetaValue::Boolean(true));
}

/// The villager state of an entity, if it is a villager.
pub fn state(store: &EntityStore, key: EntityKey) -> Option<&VillagerState> {
    store.data(key)?.villager.as_ref()
}

/// Mutable villager state of an entity.
pub fn state_mut(store: &mut EntityStore, key: EntityKey) -> Option<&mut VillagerState> {
    store.data_mut(key)?.villager.as_mut()
}

/// The offers of a villager entity.
pub fn offers(store: &EntityStore, key: EntityKey) -> Option<&[Trade]> {
    Some(state(store, key)?.offers.as_slice())
}

/// Whether another villager in `store` owns the workstation at `position`.
fn workstation_claimed(store: &EntityStore, owner: EntityKey, position: [i32; 3]) -> bool {
    store.keys().iter().copied().any(|key| {
        key != owner
            && store
                .data(key)
                .and_then(|data| data.villager.as_ref())
                .is_some_and(|villager| villager.workstation == Some(position))
    })
}

/// The nearest unclaimed workstation around `origin` that assigns `wanted` (or any profession).
pub fn nearest_workstation(
    store: &EntityStore,
    owner: EntityKey,
    blocks: &impl BlockGetter,
    origin: Vec3,
    wanted: Option<Profession>,
) -> Option<(Profession, [i32; 3])> {
    let ox = origin.x.floor() as i32;
    let oy = origin.y.floor() as i32;
    let oz = origin.z.floor() as i32;
    let mut best: Option<(i32, Profession, [i32; 3])> = None;
    for dx in -WORKSTATION_RADIUS..=WORKSTATION_RADIUS {
        for dz in -WORKSTATION_RADIUS..=WORKSTATION_RADIUS {
            for dy in -WORKSTATION_VERTICAL..=WORKSTATION_VERTICAL {
                let position = [ox + dx, oy + dy, oz + dz];
                let Some(block) = blocks.block_state(position[0], position[1], position[2]) else {
                    continue;
                };
                let Some((name, _)) = ferrite_data::blocks::state(block) else {
                    continue;
                };
                let Some(profession) = Profession::from_workstation(name) else {
                    continue;
                };
                if wanted.is_some_and(|wanted| wanted != profession)
                    || workstation_claimed(store, owner, position)
                {
                    continue;
                }
                let distance = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(same, _, _)| distance < same) {
                    best = Some((distance, profession, position));
                }
            }
        }
    }
    best.map(|(_, profession, position)| (profession, position))
}

/// Whether the block at `position` still assigns `profession`.
fn workstation_matches(
    blocks: &impl BlockGetter,
    position: [i32; 3],
    profession: Profession,
) -> bool {
    blocks
        .block_state(position[0], position[1], position[2])
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, _)| Profession::from_workstation(name) == Some(profession))
}

/// The foot position of a bed block, canonicalizing a head half by its facing.
fn bed_foot(blocks: &impl BlockGetter, position: [i32; 3]) -> Option<[i32; 3]> {
    let (name, props) = blocks
        .block_state(position[0], position[1], position[2])
        .and_then(ferrite_data::blocks::state)?;
    if !name.ends_with("_bed") {
        return None;
    }
    let part = props
        .iter()
        .find_map(|(key, value)| (*key == "part").then_some(*value));
    if part != Some("head") {
        return Some(position);
    }
    let facing = props
        .iter()
        .find_map(|(key, value)| (*key == "facing").then_some(*value))?;
    let (dx, dz) = match facing {
        "north" => (0, 1),
        "south" => (0, -1),
        "west" => (1, 0),
        "east" => (-1, 0),
        _ => return None,
    };
    Some([position[0] + dx, position[1], position[2] + dz])
}

/// Whether another villager in `store` owns the bed at `position`.
fn bed_claimed(store: &EntityStore, owner: EntityKey, position: [i32; 3]) -> bool {
    store.keys().iter().copied().any(|key| {
        key != owner
            && store
                .data(key)
                .and_then(|data| data.villager.as_ref())
                .is_some_and(|villager| villager.bed == Some(position))
    })
}

/// The nearest unclaimed bed foot around `origin`.
pub fn nearest_bed(
    store: &EntityStore,
    owner: EntityKey,
    blocks: &impl BlockGetter,
    origin: Vec3,
) -> Option<[i32; 3]> {
    let ox = origin.x.floor() as i32;
    let oy = origin.y.floor() as i32;
    let oz = origin.z.floor() as i32;
    let mut best: Option<(i32, [i32; 3])> = None;
    for dx in -BED_RADIUS..=BED_RADIUS {
        for dz in -BED_RADIUS..=BED_RADIUS {
            for dy in -BED_VERTICAL..=BED_VERTICAL {
                let Some(foot) = bed_foot(blocks, [ox + dx, oy + dy, oz + dz]) else {
                    continue;
                };
                if bed_claimed(store, owner, foot) {
                    continue;
                }
                let distance =
                    (foot[0] - ox).pow(2) + (foot[1] - oy).pow(2) + (foot[2] - oz).pow(2);
                if best.is_none_or(|(same, _)| distance < same) {
                    best = Some((distance, foot));
                }
            }
        }
    }
    best.map(|(_, foot)| foot)
}

/// The nearest bell around `origin`.
pub fn nearest_bell(blocks: &impl BlockGetter, origin: Vec3) -> Option<[i32; 3]> {
    let ox = origin.x.floor() as i32;
    let oy = origin.y.floor() as i32;
    let oz = origin.z.floor() as i32;
    let mut best: Option<(i32, [i32; 3])> = None;
    for dx in -BELL_RADIUS..=BELL_RADIUS {
        for dz in -BELL_RADIUS..=BELL_RADIUS {
            for dy in -BELL_VERTICAL..=BELL_VERTICAL {
                let position = [ox + dx, oy + dy, oz + dz];
                let Some(block) = blocks.block_state(position[0], position[1], position[2]) else {
                    continue;
                };
                let Some((name, _)) = ferrite_data::blocks::state(block) else {
                    continue;
                };
                if name.strip_prefix("minecraft:").unwrap_or(name) != "bell" {
                    continue;
                }
                let distance = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(same, _)| distance < same) {
                    best = Some((distance, position));
                }
            }
        }
    }
    best.map(|(_, position)| position)
}

/// Vanilla job-site reach: within eight blocks horizontally and at most nine below the feet.
pub fn workstation_reached(pos: Vec3, workstation: [i32; 3]) -> bool {
    let dx = pos.x - (f64::from(workstation[0]) + 0.5);
    let dz = pos.z - (f64::from(workstation[2]) + 0.5);
    let y = f64::from(workstation[1]);
    dx * dx + dz * dz <= WORK_REACH_HORIZONTAL.powi(2)
        && y <= pos.y + 1.0
        && y >= pos.y - WORK_REACH_BELOW
}

/// Whether the block a villager would step into while walking along `yaw` is water or lava.
pub fn ahead_is_liquid(blocks: &impl BlockGetter, pos: Vec3, yaw: f32) -> bool {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let x = (pos.x - sin * 1.2).floor() as i32;
    let z = (pos.z + cos * 1.2).floor() as i32;
    for y in [pos.y.floor() as i32, pos.y.floor() as i32 - 1] {
        if let Some(block) = blocks.block_state(x, y, z)
            && ferrite_data::block_props::get(block).fluid.is_some()
        {
            return true;
        }
    }
    false
}

/// Ticks one villager's schedule, job/bed claims, movement target and restocking.
///
/// `day_time` is the world clock (`GameState.clock_time`) and `game_time` is total world ticks;
/// restock spacing uses the latter, like vanilla's `LastRestock`.
pub fn tick(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    day_time: i64,
    game_time: i64,
) {
    let Some(data) = store.data(key) else {
        return;
    };
    let Some(current) = data.villager.clone() else {
        return;
    };
    let Some(mob) = data.mob.as_ref() else {
        return;
    };
    // A wandering trader keeps its offers and never takes a job.
    if mob.kind != MobKind::Villager {
        return;
    }
    let baby = mob.age < 0;
    let uuid = data.uuid;
    let pos = store.pos(key).unwrap_or(Vec3::ZERO);
    let mut state = current;
    let before = (state.profession, state.level, state.villager_type);
    let day = day_time.div_euclid(24_000);
    if state.restock_day != day {
        state.restocks_today = 0;
        state.restock_day = day;
    }
    if state.last_restock >= 0
        && game_time.saturating_sub(state.last_restock) > RESTOCK_FORGET_TICKS
    {
        state.restocks_today = 0;
    }

    // A remembered workstation is valid only while it still assigns the profession and no
    // other villager in this region owns it.
    if let Some(workstation) = state.workstation
        && (!workstation_matches(blocks, workstation, state.profession)
            || workstation_claimed(store, key, workstation))
    {
        state.workstation = None;
    }
    if baby || state.profession == Profession::Nitwit {
        state.workstation = None;
    }
    let needs_workstation = !baby
        && state.profession != Profession::Nitwit
        && (state.profession == Profession::None || state.workstation.is_none());
    let mut searched = false;
    if needs_workstation && state.search_timer == 0 {
        let wanted = (state.profession != Profession::None).then_some(state.profession);
        if let Some((profession, workstation)) =
            nearest_workstation(store, key, blocks, pos, wanted)
        {
            if state.profession == Profession::None {
                state.profession = profession;
                state.offers = offers_for(profession, state.level, uuid);
            }
            state.workstation = Some(workstation);
        }
        state.search_timer = WORKSTATION_SEARCH_TICKS;
        searched = true;
    }

    let employed = !baby && !matches!(state.profession, Profession::None | Profession::Nitwit);
    let activity = activity_at(day_time, baby, employed);
    if let Some(bed) = state.bed
        && bed_foot(blocks, bed) != Some(bed)
    {
        state.bed = None;
    }
    if activity == Activity::Rest
        && state.bed.is_none()
        && (state.search_timer == 0 || searched)
        && let Some(bed) = nearest_bed(store, key, blocks, pos)
    {
        state.bed = Some(bed);
        state.search_timer = WORKSTATION_SEARCH_TICKS;
    }
    if state.search_timer > 0 {
        state.search_timer -= 1;
    }

    let home = [
        state.home[0].floor() as i32,
        state.home[1].floor() as i32,
        state.home[2].floor() as i32,
    ];
    if activity == Activity::Meet {
        if state.activity != Activity::Meet || state.meeting_point.is_none() {
            state.meeting_point = nearest_bell(blocks, pos).or(Some(home));
        }
    } else {
        state.meeting_point = None;
    }
    state.destination = match activity {
        Activity::Work => state.workstation,
        Activity::Rest => state.bed.or(Some(home)),
        Activity::Meet => state.meeting_point,
        Activity::Idle | Activity::Play => None,
    };

    // Vanilla `Villager.shouldRestock` plus the two-per-day `RestocksToday` cap. A restock
    // happens while working at a valid job site and only refreshes offers that sold out.
    if !baby
        && activity == Activity::Work
        && state.restocks_today < RESTOCKS_PER_DAY
        && (state.last_restock < 0
            || game_time.saturating_sub(state.last_restock) >= RESTOCK_COOLDOWN_TICKS)
        && let Some(workstation) = state.workstation
        && workstation_reached(pos, workstation)
        && state.offers.iter().any(|trade| trade.is_out_of_stock())
    {
        for trade in &mut state.offers {
            trade.restock();
        }
        state.last_restock = game_time;
        state.restocks_today = state.restocks_today.saturating_add(1);
    }

    state.activity = activity;
    if let Some(data) = store.data_mut(key) {
        let identity_changed = before != (state.profession, state.level, state.villager_type);
        data.villager = Some(state.clone());
        if identity_changed {
            sync_metadata(&mut data.metadata, &state);
        }
    }
}

/// Ticks every villager in one region. Called once per region tick before mob AI.
pub fn tick_all(store: &mut EntityStore, blocks: &impl BlockGetter, day_time: i64, game_time: i64) {
    for key in store.keys().to_vec() {
        tick(store, key, blocks, day_time, game_time);
    }
}

/// Writes a menu's offers and villager XP back into the entity after a trade, leveling the
/// villager up when the trade XP crosses vanilla's thresholds.
pub fn apply_trades(store: &mut EntityStore, key: EntityKey, trades: Vec<Trade>, xp: i32) {
    let Some(data) = store.data(key) else {
        return;
    };
    let uuid = data.uuid;
    let Some(current) = data.villager.clone() else {
        return;
    };
    let mut state = current;
    state.offers = trades;
    gain_trade_xp(&mut state, xp, uuid);
    if let Some(data) = store.data_mut(key) {
        data.villager = Some(state.clone());
        sync_metadata(&mut data.metadata, &state);
    }
}

/// Adds trade XP earned through an already-merged menu, leveling up as thresholds are crossed.
pub fn add_trade_xp(store: &mut EntityStore, key: EntityKey, xp: i32) {
    let Some(data) = store.data(key) else {
        return;
    };
    let uuid = data.uuid;
    let Some(current) = data.villager.clone() else {
        return;
    };
    if xp <= 0 {
        return;
    }
    let mut state = current;
    gain_trade_xp(&mut state, xp, uuid);
    if let Some(data) = store.data_mut(key) {
        data.villager = Some(state.clone());
        sync_metadata(&mut data.metadata, &state);
    }
}

fn gain_trade_xp(state: &mut VillagerState, xp: i32, uuid: Uuid) {
    state.xp = state.xp.saturating_add(xp.max(0));
    while state.level < MAX_LEVEL {
        let threshold = NEXT_LEVEL_XP[(state.level - 1) as usize];
        if state.xp < threshold {
            break;
        }
        state.level += 1;
        state
            .offers
            .extend(level_offers(state.profession, state.level, uuid));
    }
}

/// The offers of levels `1..=level`, in level order.
pub fn offers_for(profession: Profession, level: u8, uuid: Uuid) -> Vec<Trade> {
    let mut offers = Vec::new();
    for level in 1..=level.clamp(1, MAX_LEVEL) {
        offers.extend(level_offers(profession, level, uuid));
    }
    offers
}

/// `set.amount` distinct offers drawn deterministically from one profession level's pool.
fn level_offers(profession: Profession, level: u8, uuid: Uuid) -> Vec<Trade> {
    if matches!(profession, Profession::None | Profession::Nitwit) {
        return Vec::new();
    }
    draw(profession.name(), level, seed(uuid, profession.id(), level))
}

/// The wandering trader's offers: the `common`, `uncommon` and `buying` trade sets of the
/// data (5, 2 and 2 offers), drawn deterministically from its uuid.
pub fn trader_offers(uuid: Uuid) -> Vec<Trade> {
    (1..=3u8)
        .flat_map(|level| draw("wandering_trader", level, seed(uuid, 99, level)))
        .collect()
}

/// `set.amount` distinct offers drawn from the pool named `set_name` at `level`.
fn draw(set_name: &str, level: u8, seed: u64) -> Vec<Trade> {
    let Some(set) = ferrite_data::villager_trades::set(set_name, level) else {
        return Vec::new();
    };
    let mut pool: Vec<&'static str> = set.trades.to_vec();
    let mut rng = seed;
    let mut out = Vec::new();
    for _ in 0..set.amount.min(pool.len()) {
        let pick = (next(&mut rng) % pool.len() as u64) as usize;
        let name = pool.swap_remove(pick);
        let Some(def) = ferrite_data::villager_trades::offer(name) else {
            continue;
        };
        let wants = ferrite_item::ItemStack::with_count(def.wants, def.wants_count);
        let gives = ferrite_item::ItemStack::with_count(def.gives, def.gives_count);
        let mut trade = Trade::new(wants, gives, def.max_uses, def.xp);
        trade.second_wants = def
            .second_wants
            .map(|(item, count)| ferrite_item::ItemStack::with_count(item, count));
        trade.price_multiplier = def.price_multiplier;
        out.push(trade);
    }
    out
}

/// Deterministic seed for one villager's offer choices: FNV-1a over the UUID plus the
/// profession and level, so offers survive a region hand-off and a restart.
fn seed(uuid: Uuid, profession: i32, level: u8) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in uuid.as_bytes().iter().chain(&profession.to_le_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash ^ u64::from(level).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// SplitMix64, for the offer draw.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Whether an entity is a villager (`MobKind::Villager`).
pub fn is_villager(store: &EntityStore, key: EntityKey) -> bool {
    crate::mob::state(store, key).is_some_and(|state| state.kind == MobKind::Villager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rng;
    use crate::mob::new_mob;
    use crate::physics::BlockGetter;

    struct Flat {
        blocks: Vec<(&'static str, [i32; 3])>,
    }

    impl Flat {
        fn empty() -> Self {
            Self { blocks: Vec::new() }
        }

        fn with(name: &'static str, at: [i32; 3]) -> Self {
            Self {
                blocks: vec![(name, at)],
            }
        }

        fn with_blocks(blocks: &[(&'static str, [i32; 3])]) -> Self {
            Self {
                blocks: blocks.to_vec(),
            }
        }
    }

    impl BlockGetter for Flat {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some((name, _)) = self.blocks.iter().find(|(_, at)| *at == [x, y, z]) {
                return ferrite_data::blocks::default_state(name);
            }
            ferrite_data::blocks::default_state(if y < 64 { "stone" } else { "air" })
        }
    }

    fn villager_at(store: &mut EntityStore, x: f64, z: f64) -> (EntityKey, Uuid) {
        let mut rng = Rng::new((x.to_bits() ^ z.to_bits()).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let key = store.insert(new_mob(MobKind::Villager, Vec3::new(x, 64.0, z), &mut rng));
        let uuid = store.data(key).unwrap().uuid;
        (key, uuid)
    }

    fn villager() -> (EntityStore, EntityKey, Uuid) {
        let mut store = EntityStore::default();
        let (key, uuid) = villager_at(&mut store, 0.5, 0.5);
        (store, key, uuid)
    }

    #[test]
    fn every_workstation_maps_to_its_profession() {
        for (block, profession) in [
            ("lectern", Profession::Librarian),
            ("composter", Profession::Farmer),
            ("blast_furnace", Profession::Armorer),
            ("smithing_table", Profession::Weaponsmith),
            ("cartography_table", Profession::Cartographer),
            ("fletching_table", Profession::Fletcher),
            ("grindstone", Profession::Toolsmith),
            ("brewing_stand", Profession::Cleric),
            ("barrel", Profession::Fisherman),
            ("cauldron", Profession::Leatherworker),
            ("loom", Profession::Shepherd),
            ("stonecutter", Profession::Mason),
            ("smoker", Profession::Butcher),
            ("stone", Profession::None),
        ] {
            assert_eq!(
                Profession::from_workstation(block),
                (profession != Profession::None).then_some(profession),
                "{block}"
            );
        }
        assert_eq!(Profession::Farmer.id(), 5);
        assert_eq!(VillagerType::Plains.id(), 2);
    }

    #[test]
    fn offers_are_deterministic_and_come_from_the_vanilla_tables() {
        let uuid = Uuid::from_u128(0x1234_5678_9abc_def0);
        let first = offers_for(Profession::Farmer, 1, uuid);
        assert_eq!(first.len(), 2, "a level-1 farmer draws two offers");
        assert_eq!(first, offers_for(Profession::Farmer, 1, uuid));
        assert_ne!(first, offers_for(Profession::Farmer, 1, Uuid::from_u128(7)));
        for trade in &first {
            assert!(!trade.gives.is_empty());
            assert!(!trade.wants.is_empty());
            assert!(trade.wants.count > 0);
            assert_eq!(trade.uses, 0);
            assert!(trade.max_uses > 0);
            // Every farmer offer is an emerald in or out.
            assert!(
                trade.wants.name() == Some("minecraft:emerald")
                    || trade.gives.name() == Some("minecraft:emerald")
            );
        }
        // Unemployed and nitwit villagers have nothing to sell.
        assert!(offers_for(Profession::None, 5, uuid).is_empty());
        assert!(offers_for(Profession::Nitwit, 5, uuid).is_empty());
        // Higher levels add two offers per level.
        assert_eq!(offers_for(Profession::Farmer, 3, uuid).len(), 6);
    }

    #[test]
    fn a_nearby_workstation_assigns_the_profession_and_metadata() {
        let (mut store, key, _) = villager();
        assert_eq!(state(&store, key).unwrap().profession, Profession::None);
        let world = Flat::with("composter", [3, 64, 0]);
        tick(&mut store, key, &world, 2000, 2000);
        let villager = state(&store, key).unwrap();
        assert_eq!(villager.profession, Profession::Farmer);
        assert_eq!(villager.workstation, Some([3, 64, 0]));
        assert_eq!(villager.offers.len(), 2);
        let data = store.data(key).unwrap();
        assert_eq!(
            data.metadata.get(VILLAGER_DATA_INDEX),
            Some(&MetaValue::VillagerData {
                kind: VillagerType::Plains.id(),
                profession: Profession::Farmer.id(),
                level: 1,
            })
        );
        assert_eq!(
            data.metadata.get(VILLAGER_DATA_FINALIZED_INDEX),
            Some(&MetaValue::Boolean(true))
        );
        // The profession and its point of interest stick even when the block is gone.
        tick(&mut store, key, &Flat::empty(), 2000, 2000);
        assert_eq!(state(&store, key).unwrap().profession, Profession::Farmer);
        assert_eq!(state(&store, key).unwrap().workstation, None);
    }

    #[test]
    fn the_java_schedule_changes_activity_at_the_documented_boundaries() {
        assert_eq!(activity_at(0, false, true), Activity::Idle);
        assert_eq!(activity_at(10, false, true), Activity::Idle);
        assert_eq!(activity_at(1999, false, true), Activity::Idle);
        assert_eq!(activity_at(2000, false, true), Activity::Work);
        assert_eq!(activity_at(8999, false, true), Activity::Work);
        assert_eq!(activity_at(9000, false, true), Activity::Meet);
        assert_eq!(activity_at(10_999, false, true), Activity::Meet);
        assert_eq!(activity_at(11_000, false, true), Activity::Idle);
        assert_eq!(activity_at(11_999, false, true), Activity::Idle);
        assert_eq!(activity_at(12_000, false, true), Activity::Rest);
        assert_eq!(activity_at(23_999, false, true), Activity::Rest);
        assert_eq!(activity_at(2000, false, false), Activity::Idle);
        assert_eq!(activity_at(9000, false, false), Activity::Meet);
        assert_eq!(activity_at(3000, true, false), Activity::Play);
        assert_eq!(activity_at(6000, true, false), Activity::Idle);
        assert_eq!(activity_at(10_000, true, false), Activity::Play);
        assert_eq!(activity_at(12_000, true, false), Activity::Rest);
    }

    #[test]
    fn a_workstation_has_only_one_owner_and_unemployment_searches_again() {
        let (mut store, first, _) = villager();
        let (second, _) = villager_at(&mut store, 1.5, 0.5);
        let world = Flat::with("composter", [2, 64, 0]);
        tick(&mut store, first, &world, 2000, 2000);
        tick(&mut store, second, &world, 2000, 2000);
        assert_eq!(state(&store, first).unwrap().profession, Profession::Farmer);
        assert_eq!(state(&store, second).unwrap().profession, Profession::None);
        assert_eq!(state(&store, second).unwrap().workstation, None);

        let world = Flat::with_blocks(&[("composter", [2, 64, 0]), ("lectern", [3, 64, 0])]);
        for _ in 0..WORKSTATION_SEARCH_TICKS {
            tick(&mut store, second, &world, 2000, 2000);
        }
        assert_eq!(
            state(&store, second).unwrap().profession,
            Profession::Librarian
        );
        assert_eq!(state(&store, second).unwrap().workstation, Some([3, 64, 0]));
    }

    #[test]
    fn a_removed_workstation_is_released_and_a_matching_one_is_claimed() {
        let (mut store, key, _) = villager();
        tick(
            &mut store,
            key,
            &Flat::with("composter", [2, 64, 0]),
            2000,
            2000,
        );
        assert_eq!(state(&store, key).unwrap().workstation, Some([2, 64, 0]));
        tick(&mut store, key, &Flat::empty(), 2000, 2000);
        assert_eq!(state(&store, key).unwrap().profession, Profession::Farmer);
        assert_eq!(state(&store, key).unwrap().workstation, None);

        let world = Flat::with("composter", [4, 64, 0]);
        for _ in 0..WORKSTATION_SEARCH_TICKS {
            tick(&mut store, key, &world, 2000, 2000);
        }
        assert_eq!(state(&store, key).unwrap().workstation, Some([4, 64, 0]));
    }

    #[test]
    fn a_bed_has_only_one_owner_and_releases_when_removed() {
        let (mut store, first, _) = villager();
        let (second, _) = villager_at(&mut store, 3.5, 0.5);
        let one_bed = Flat::with("red_bed", [2, 64, 0]);
        tick(&mut store, first, &one_bed, 12_000, 12_000);
        tick(&mut store, second, &one_bed, 12_000, 12_000);
        assert_eq!(state(&store, first).unwrap().bed, Some([2, 64, 0]));
        assert_eq!(state(&store, second).unwrap().bed, None);

        let two_beds = Flat::with_blocks(&[("red_bed", [2, 64, 0]), ("red_bed", [4, 64, 0])]);
        for _ in 0..WORKSTATION_SEARCH_TICKS {
            tick(&mut store, second, &two_beds, 12_000, 12_000);
        }
        assert_eq!(state(&store, second).unwrap().bed, Some([4, 64, 0]));

        tick(&mut store, first, &Flat::empty(), 12_000, 12_000);
        assert_eq!(state(&store, first).unwrap().bed, None);
    }

    #[test]
    fn restocking_needs_work_a_reached_job_site_and_a_sold_out_offer() {
        let (mut store, key, _) = villager();
        let world = Flat::with("composter", [2, 64, 0]);
        tick(&mut store, key, &world, 2000, 2000);
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.offers[0].uses = villager.offers[0].max_uses;
            villager.offers[0].demand = 4;
            villager.offers[0].special_price = 3;
            villager.last_restock = -RESTOCK_COOLDOWN_TICKS;
            villager.restocks_today = 0;
        }
        tick(&mut store, key, &world, 2000, 2000);
        let villager = state(&store, key).unwrap();
        assert_eq!(villager.restocks_today, 1);
        assert_eq!(villager.last_restock, 2000);
        assert_eq!(villager.offers[0].uses, 0);
        assert_eq!(villager.offers[0].demand, 0);
        assert_eq!(villager.offers[0].special_price, 0);

        // A second restock is refused inside the 2400-tick cooldown.
        state_mut(&mut store, key).unwrap().offers[0].uses =
            state(&store, key).unwrap().offers[0].max_uses;
        tick(&mut store, key, &world, 4000, 4000);
        assert_eq!(state(&store, key).unwrap().restocks_today, 1);
        tick(&mut store, key, &world, 4400, 4400);
        assert_eq!(state(&store, key).unwrap().restocks_today, 2);

        // The two-per-day cap wins over the cooldown.
        state_mut(&mut store, key).unwrap().offers[0].uses =
            state(&store, key).unwrap().offers[0].max_uses;
        tick(&mut store, key, &world, 7000, 7000);
        assert_eq!(state(&store, key).unwrap().restocks_today, 2);

        // A new day resets the counter.
        state_mut(&mut store, key).unwrap().offers[0].uses =
            state(&store, key).unwrap().offers[0].max_uses;
        tick(&mut store, key, &world, 26_000, 26_000);
        assert_eq!(state(&store, key).unwrap().restocks_today, 1);

        // Night or a missing job site cannot restock.
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.offers[0].uses = villager.offers[0].max_uses;
            villager.restocks_today = 1;
            villager.last_restock = 39_000;
        }
        tick(&mut store, key, &world, 36_000, 40_000);
        assert_eq!(state(&store, key).unwrap().restocks_today, 1);
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.offers[0].uses = villager.offers[0].max_uses;
            villager.restocks_today = 1;
            villager.last_restock = 59_000;
        }
        tick(&mut store, key, &Flat::empty(), 26_000, 60_000);
        assert_eq!(state(&store, key).unwrap().restocks_today, 1);
    }

    #[test]
    fn restocks_today_decays_after_ten_minutes_without_a_restock() {
        let (mut store, key, _) = villager();
        let world = Flat::with("composter", [2, 64, 0]);
        tick(&mut store, key, &world, 2000, 2000);
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.offers[0].uses = villager.offers[0].max_uses;
            villager.last_restock = 0;
            villager.restocks_today = 2;
            villager.restock_day = 0;
        }
        tick(&mut store, key, &world, 2000, 12_001);
        let villager = state(&store, key).unwrap();
        assert_eq!(villager.restocks_today, 1);
        assert_eq!(villager.last_restock, 12_001);
    }

    #[test]
    fn job_site_reach_matches_the_vanilla_eight_by_nine_box() {
        let feet = Vec3::new(0.5, 64.0, 0.5);
        assert!(workstation_reached(feet, [8, 64, 0]));
        assert!(!workstation_reached(feet, [9, 64, 0]));
        assert!(workstation_reached(feet, [0, 55, 0]));
        assert!(!workstation_reached(feet, [0, 54, 0]));
        assert!(workstation_reached(feet, [0, 65, 0]));
    }

    #[test]
    fn a_completed_trade_adds_levels_and_offers() {
        let (mut store, key, _) = villager();
        tick(
            &mut store,
            key,
            &Flat::with("composter", [1, 64, 0]),
            2000,
            2000,
        );
        add_trade_xp(&mut store, key, NEXT_LEVEL_XP[0]);
        let villager = state(&store, key).unwrap();
        assert_eq!(villager.level, 2);
        assert_eq!(villager.xp, NEXT_LEVEL_XP[0]);
        assert_eq!(villager.offers.len(), 4);
        assert!(matches!(
            store.data(key).unwrap().metadata.get(VILLAGER_DATA_INDEX),
            Some(MetaValue::VillagerData { level: 2, .. })
        ));
    }

    #[test]
    fn walking_into_water_or_lava_is_detected() {
        struct Pool;
        impl BlockGetter for Pool {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                ferrite_data::blocks::default_state(if y < 64 {
                    "stone"
                } else if y == 64 {
                    "water"
                } else {
                    "air"
                })
            }
        }
        assert!(ahead_is_liquid(&Pool, Vec3::new(0.5, 64.0, 0.5), 0.0));
        assert!(!ahead_is_liquid(
            &Flat::empty(),
            Vec3::new(0.5, 64.0, 0.5),
            0.0
        ));
    }

    #[test]
    fn villager_metadata_indices_match_the_vanilla_layout() {
        assert_eq!(VILLAGER_DATA_INDEX, 19);
        assert_eq!(VILLAGER_DATA_FINALIZED_INDEX, 20);
        assert_eq!(VILLAGER_DATA_SERIALIZER, 18);
        // The villager's health comes from the vanilla attributes.
        assert_eq!(MobKind::Villager.max_health(), 20.0);
        assert!(!MobKind::Villager.is_hostile());
        assert_eq!(MobKind::from_name("villager"), Some(MobKind::Villager));
    }

    #[test]
    fn a_wandering_trader_draws_its_offers_from_the_three_pools() {
        let uuid = Uuid::from_u128(77);
        let offers = trader_offers(uuid);
        assert_eq!(offers.len(), 9, "5 common, 2 uncommon and 2 buying");
        assert_eq!(offers, trader_offers(uuid), "the draw is deterministic");
        assert_ne!(offers, trader_offers(Uuid::from_u128(78)));
    }

    fn priced_trade(base: i32) -> Trade {
        let wants = ferrite_item::ItemStack::from_name("emerald")
            .expect("emerald")
            .copy_with_count(base);
        let gives = ferrite_item::ItemStack::from_name("bread")
            .expect("bread")
            .copy_with_count(1);
        let mut trade = Trade::new(wants, gives, 12, 2);
        trade.price_multiplier = 0.05;
        trade
    }

    fn emeralds(count: i32) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::from_name("emerald")
            .expect("emerald")
            .copy_with_count(count)
    }

    #[test]
    fn registry_names_round_trip_for_saving() {
        for profession in [
            Profession::None,
            Profession::Farmer,
            Profession::Librarian,
            Profession::Weaponsmith,
        ] {
            assert_eq!(Profession::from_name(profession.name()), Some(profession));
        }
        assert_eq!(Profession::from_name("farmer"), Some(Profession::Farmer));
        assert_eq!(
            VillagerType::from_name("minecraft:snow"),
            Some(VillagerType::Snow)
        );
        assert_eq!(VillagerType::from_name("minecraft:moon"), None);
        assert_eq!(
            GossipType::from_name("major_negative"),
            Some(GossipType::MajorNegative)
        );
        assert_eq!(
            GossipType::from_name("minecraft:trading"),
            Some(GossipType::Trading)
        );
        assert_eq!(GossipType::from_name("unknown"), None);
    }

    #[test]
    fn gossip_is_weighted_capped_and_kept_per_player() {
        let mut gossips = Gossips::default();
        let player = Uuid::from_u128(7);
        gossips.add(player, GossipType::Trading, 100);
        assert_eq!(
            gossips.get(player, GossipType::Trading),
            GossipType::Trading.max(),
            "trading gossip is capped at 25"
        );
        assert_eq!(gossips.reputation(player), 25);
        gossips.add(player, GossipType::MinorPositive, 25);
        gossips.add(player, GossipType::MinorNegative, 25);
        gossips.add(player, GossipType::MajorPositive, 20);
        gossips.add(player, GossipType::MajorNegative, 20);
        // 5*20 + 25 + 25 - 25 - 5*20 = 25
        assert_eq!(gossips.reputation(player), 25);
        assert_eq!(gossips.reputation(Uuid::from_u128(8)), 0);
        assert_eq!(gossips.entries().count(), 5);
        assert_eq!(GossipType::MajorPositive.weight(), 5);
        assert_eq!(GossipType::MajorNegative.weight(), -5);
        assert_eq!(GossipType::MajorPositive.max(), 20);
        assert_eq!(GossipType::MinorNegative.max(), 200);
    }

    #[test]
    fn reputation_discounts_and_surcharges_are_per_player() {
        let (mut store, key, _) = villager();
        let alice = Uuid::from_u128(1);
        let bob = Uuid::from_u128(2);
        state_mut(&mut store, key).unwrap().offers = vec![priced_trade(32)];

        // Neutral reputation leaves the base price.
        let neutral = state(&store, key).unwrap().offers_for_player(bob);
        assert_eq!(neutral[0].price(), 32);
        assert_eq!(neutral[0].special_price, 0);

        // A cure's major positive gossip: 20 points x weight 5 = 100 reputation, and
        // floor(100 x 0.05) is a five-emerald discount.
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.gossips.add(alice, GossipType::MajorPositive, 20);
            assert_eq!(villager.reputation(alice), 100);
        }
        let discounted = state(&store, key).unwrap().offers_for_player(alice);
        assert_eq!(discounted[0].special_price, -5);
        assert_eq!(discounted[0].price(), 27);

        // Killing a villager's major negative gossip raises bob's price by the same amount.
        state_mut(&mut store, key)
            .unwrap()
            .gossips
            .add(bob, GossipType::MajorNegative, 20);
        assert_eq!(state(&store, key).unwrap().reputation(bob), -100);
        let surcharged = state(&store, key).unwrap().offers_for_player(bob);
        assert_eq!(surcharged[0].special_price, 5);
        assert_eq!(surcharged[0].price(), 37);

        // The stored offer is untouched, so a third player still sees the base price.
        assert_eq!(state(&store, key).unwrap().offers[0].special_price, 0);
        assert_eq!(
            state(&store, key).unwrap().offers_for_player(alice)[0].price(),
            27
        );
    }

    #[test]
    fn every_completed_trade_adds_trading_gossip_up_to_the_cap() {
        let (mut store, key, _) = villager();
        let player = Uuid::from_u128(3);
        for _ in 0..3 {
            state_mut(&mut store, key).unwrap().record_trade(player);
        }
        assert_eq!(
            state(&store, key)
                .unwrap()
                .gossips
                .get(player, GossipType::Trading),
            6
        );
        for _ in 0..20 {
            state_mut(&mut store, key).unwrap().record_trade(player);
        }
        assert_eq!(
            state(&store, key)
                .unwrap()
                .gossips
                .get(player, GossipType::Trading),
            GossipType::Trading.max()
        );
        assert_eq!(
            state(&store, key)
                .unwrap()
                .gossips
                .get(Uuid::from_u128(4), GossipType::Trading),
            0,
            "gossip is per player"
        );
    }

    #[test]
    fn a_trade_validates_against_the_displayed_reputation_price() {
        let (mut store, key, _) = villager();
        let player = Uuid::from_u128(4);
        let mut trade = priced_trade(32);
        trade.price_multiplier = 0.1;
        state_mut(&mut store, key).unwrap().offers = vec![trade];
        state_mut(&mut store, key)
            .unwrap()
            .gossips
            .add(player, GossipType::MajorPositive, 20);
        let offers = state(&store, key).unwrap().offers_for_player(player);
        let shown = offers[0].price();
        assert_eq!(shown, 22, "100 reputation x 0.1 is ten emeralds off");
        assert!(
            !offers[0].satisfied_by(&emeralds(shown - 1), &ferrite_item::ItemStack::empty()),
            "paying less than the shown price must not complete the trade"
        );
        assert!(offers[0].satisfied_by(&emeralds(shown), &ferrite_item::ItemStack::empty()));
    }

    #[test]
    fn trade_state_and_reputation_move_with_the_entity_record() {
        let (mut store, key, _) = villager();
        tick(
            &mut store,
            key,
            &Flat::with("composter", [1, 64, 0]),
            2000,
            2000,
        );
        let player = Uuid::from_u128(5);
        {
            let villager = state_mut(&mut store, key).unwrap();
            villager.offers = vec![priced_trade(32)];
            villager.gossips.add(player, GossipType::MajorPositive, 20);
            villager.record_trade(player);
        }
        let record = store.remove(key).expect("the villager record");
        let mut other = EntityStore::default();
        let moved = other.insert(record);
        let villager = state(&other, moved).expect("the villager state moved");
        assert_eq!(villager.profession, Profession::Farmer);
        assert_eq!(villager.offers.len(), 1);
        assert_eq!(villager.gossips.get(player, GossipType::MajorPositive), 20);
        assert_eq!(
            villager.gossips.get(player, GossipType::Trading),
            TRADING_GOSSIP
        );
        assert_eq!(villager.reputation(player), 102);
        assert_eq!(villager.offers_for_player(player)[0].price(), 27);
    }
}
