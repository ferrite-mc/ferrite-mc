//! Villagers: professions, workstations, trade offers, demand pricing and restocking.
//!
//! The offer pools come from the 26.3 data generator (`ferrite_data::villager_trades`), so a
//! farmer's level-1 pool is the real wheat/potato/carrot/beetroot/bread list. Each villager
//! draws its offers deterministically from its UUID, so the same villager always offers the
//! same trades.
//!
//! Simplifications, documented rather than hidden:
//! - A profession is taken from the nearest workstation block within 16 blocks (and four
//!   blocks vertically) the first time the villager is ticked. Villagers never level up from
//!   working; they level up from completed trades (vanilla's trade XP thresholds).
//! - A villager restocks every 24000 ticks while it remembers a workstation, instead of the
//!   vanilla work-schedule/POTENTIAL_JOB_SITE rules.
//! - Offers with a `merchant_predicate` are skipped, and results whose data attaches item
//!   modifiers keep their base item (see `ferrite_data::villager_trades`).
//! - The villager type is always plains.

use crate::EntityKey;
use crate::mob::MobKind;
use crate::physics::BlockGetter;
use crate::store::EntityStore;
use ferrite_inventory::Trade;
use ferrite_math::Vec3;
use ferrite_protocol::packets::play::MetaValue;
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
/// Vanilla `Villager` restock interval (24000 ticks).
pub const RESTOCK_TICKS: u32 = 24000;
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
    /// The villager's offers, with their uses and demand.
    pub offers: Vec<Trade>,
    /// Ticks until the next restock.
    pub restock_timer: u32,
    /// Ticks until the next workstation search.
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
            offers: Vec::new(),
            restock_timer: RESTOCK_TICKS,
            search_timer: 0,
            home,
        }
    }

    /// The offers a villager of `profession` and `level` draws, levels 1..=level.
    pub fn generate_offers(&self, uuid: Uuid) -> Vec<Trade> {
        offers_for(self.profession, self.level, uuid)
    }
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

/// The nearest workstation block around `origin`, with the profession it assigns.
pub fn nearest_workstation(
    blocks: &impl BlockGetter,
    origin: Vec3,
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
                let distance = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(same, _, _)| distance < same) {
                    best = Some((distance, profession, position));
                }
            }
        }
    }
    best.map(|(_, profession, position)| (profession, position))
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

/// Ticks a villager's profession search and restock timer.
///
/// Unemployed villagers look for a workstation every [`WORKSTATION_SEARCH_TICKS`] ticks; the
/// first one found assigns the profession, the offers and the point of interest. A villager
/// that remembers a workstation restocks its offers every [`RESTOCK_TICKS`].
pub fn tick(store: &mut EntityStore, key: EntityKey, blocks: &impl BlockGetter) {
    let Some(data) = store.data(key) else {
        return;
    };
    let Some(current) = data.villager.clone() else {
        return;
    };
    // A wandering trader keeps its offers and never takes a job.
    if crate::mob::state(store, key).is_some_and(|s| s.kind != MobKind::Villager) {
        return;
    }
    let uuid = data.uuid;
    let pos = store.pos(key).unwrap_or(Vec3::ZERO);
    let mut state = current;
    if state.profession == Profession::None {
        if state.search_timer == 0 {
            if let Some((profession, workstation)) = nearest_workstation(blocks, pos) {
                state.profession = profession;
                state.workstation = Some(workstation);
                state.offers = offers_for(profession, state.level, uuid);
            }
            state.search_timer = WORKSTATION_SEARCH_TICKS;
        } else {
            state.search_timer -= 1;
        }
    }
    if state.workstation.is_some() {
        state.restock_timer = state.restock_timer.saturating_sub(1);
        if state.restock_timer == 0 {
            for trade in &mut state.offers {
                trade.restock();
            }
            state.restock_timer = RESTOCK_TICKS;
        }
    }
    if let Some(data) = store.data_mut(key) {
        data.villager = Some(state.clone());
        sync_metadata(&mut data.metadata, &state);
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
    if let Some(data) = store.data_mut(key) {
        data.villager = Some(state.clone());
        sync_metadata(&mut data.metadata, &state);
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
        feature: Option<(&'static str, [i32; 3])>,
    }

    impl Flat {
        fn empty() -> Self {
            Self { feature: None }
        }

        fn with(name: &'static str, at: [i32; 3]) -> Self {
            Self {
                feature: Some((name, at)),
            }
        }
    }

    impl BlockGetter for Flat {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some((name, at)) = self.feature
                && at == [x, y, z]
            {
                return ferrite_data::blocks::default_state(name);
            }
            ferrite_data::blocks::default_state(if y < 64 { "stone" } else { "air" })
        }
    }

    fn villager() -> (EntityStore, EntityKey, Uuid) {
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            &mut Rng::new(1),
        ));
        let uuid = store.data(key).unwrap().uuid;
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
        tick(&mut store, key, &world);
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
        tick(&mut store, key, &Flat::empty());
        assert_eq!(state(&store, key).unwrap().profession, Profession::Farmer);
    }

    #[test]
    fn trades_level_the_villager_up_and_restock_at_the_workstation() {
        let (mut store, key, uuid) = villager();
        let world = Flat::with("composter", [1, 64, 0]);
        tick(&mut store, key, &world);
        let mut offers = offers_for(Profession::Farmer, 1, uuid);
        for trade in &mut offers {
            trade.uses = trade.max_uses;
        }
        apply_trades(&mut store, key, offers, NEXT_LEVEL_XP[0]);
        let villager = state(&store, key).unwrap();
        assert_eq!(villager.level, 2);
        assert_eq!(villager.xp, 10);
        assert_eq!(villager.offers.len(), 4);
        let metadata = store.data(key).unwrap().metadata.get(VILLAGER_DATA_INDEX);
        assert!(matches!(
            metadata,
            Some(MetaValue::VillagerData { level: 2, .. })
        ));
        // The restock timer only runs down at a workstation; when it hits zero every offer is
        // usable again.
        for trade in &mut state_mut(&mut store, key).unwrap().offers {
            trade.uses = trade.max_uses;
        }
        state_mut(&mut store, key).unwrap().restock_timer = 1;
        tick(&mut store, key, &world);
        assert!(
            state(&store, key)
                .unwrap()
                .offers
                .iter()
                .all(|trade| trade.uses == 0),
            "restocked"
        );
        assert_eq!(state(&store, key).unwrap().restock_timer, RESTOCK_TICKS);
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
}
