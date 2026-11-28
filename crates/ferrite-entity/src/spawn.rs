//! Natural spawning of the passive animals in loaded chunks.
//!
//! Follows vanilla `NaturalSpawner` for the creature category in the essentials: creatures
//! spawn at the surface on grass in daylight (light level above eight), at least 24 blocks
//! from every player and within 128 blocks of one, in a herd of one to three, and only while
//! the vanilla creature cap for the spawnable chunk area is not reached. The world supplies
//! blocks and light through [`SpawnEnv`]; the caller inserts the returned records into its
//! region store.

use crate::EquipmentSlot;
use crate::mob::{self, MobKind, new_mob};
use crate::physics::BlockGetter;
use crate::rng::Rng;
use crate::store::{EntityRecord, EntityStore, Flags};
use ferrite_math::Vec3;

/// Vanilla `MobCategory.CREATURE` cap per 289 spawnable chunks.
pub const CREATURE_CAP_PER_PLAYER: usize = 10;
/// Vanilla minimum spawn distance from a player.
pub const MIN_SPAWN_DISTANCE: f64 = 24.0;
/// Vanilla maximum spawn distance (also the creature despawn distance).
pub const MAX_SPAWN_DISTANCE: f64 = 128.0;
/// Vanilla animals spawn where the light level is above eight.
pub const MIN_LIGHT: u8 = 9;
/// Vanilla `MobCategory.MONSTER` cap per 289 spawnable chunks.
pub const MONSTER_CAP_PER_PLAYER: usize = 70;
/// Vanilla monsters need a light level at or below seven.
pub const MONSTER_MAX_LIGHT: u8 = 7;
/// How far below the surface the monster spawn search looks for a dark floor.
const MONSTER_DEPTH: i32 = 32;
/// Highest block the surface search looks at.
const MAX_SURFACE_Y: i32 = 96;
/// Lowest block the surface search looks at.
const MIN_SURFACE_Y: i32 = -64;
/// Vanilla `SpawnState` divides the per-category cap by this many spawnable chunks.
const SPAWN_CHUNK_AREA: usize = 17 * 17;

/// Vanilla `SpawnState.canSpawnForCategory`: the category cap is per 17x17 chunks of
/// spawnable area, not per player. Counting players would multiply the cap by every viewer
/// sharing the same spawn chunks on a crowded server.
fn category_cap(per_chunk_area: usize, spawnable_chunks: usize) -> usize {
    per_chunk_area.saturating_mul(spawnable_chunks) / SPAWN_CHUNK_AREA
}

/// The block and light queries natural spawning needs.
pub trait SpawnEnv: BlockGetter {
    /// The light level at a block, as vanilla's `getMaxLocalRawBrightness`.
    fn light(&self, x: i32, y: i32, z: i32) -> u8;

    /// The biome's creature spawn list at a column (`natural_mob_spawns`), or `None` when the
    /// world has no biome data, which spawns the four farm animals everywhere.
    fn creature_table(&self, _x: i32, _z: i32) -> Option<&[SpawnEntry]> {
        None
    }

    /// The biome's monster spawn list, as [`Self::creature_table`].
    fn monster_table(&self, _x: i32, _z: i32) -> Option<&[SpawnEntry]> {
        None
    }

    /// Whether the sky is clear directly above a block, as vanilla's `Level.canSeeSky`.
    /// Phantoms only spawn under open sky; environments that cannot answer keep the old
    /// behaviour of assuming it is clear.
    fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> bool {
        true
    }

    /// The block light level at a block (`Level.getBrightness(LightLayer.BLOCK, ...)`).
    /// Patrol spawning reads block light only, so a sunlit surface does not block it; the
    /// default falls back to [`Self::light`] for environments that cannot separate the two.
    fn block_light(&self, x: i32, y: i32, z: i32) -> u8 {
        self.light(x, y, z)
    }
}

/// One weighted entry of a biome's spawn list: the kind and the group size range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEntry {
    pub kind: MobKind,
    pub weight: i32,
    pub min: i32,
    pub max: i32,
}

/// Picks an entry by weight (vanilla `WeightedRandomList.getRandom`).
fn pick(table: &[SpawnEntry], rng: &mut Rng) -> Option<SpawnEntry> {
    let total: i32 = table.iter().map(|entry| entry.weight.max(0)).sum();
    if total <= 0 {
        return None;
    }
    let mut roll = rng.next_int(total);
    table.iter().copied().find(|entry| {
        roll -= entry.weight.max(0);
        roll < 0
    })
}

/// Number of live passive creatures in a store (excludes hostile monsters so underground
/// monsters never starve the passive animal cap).
pub fn creature_count(store: &EntityStore) -> usize {
    store
        .keys()
        .iter()
        .filter(|key| {
            store
                .data(**key)
                .is_some_and(|data| data.mob.is_some_and(|mob| !mob.kind.is_hostile()))
                && store
                    .flags(**key)
                    .is_some_and(|flags| !flags.has(Flags::REMOVED))
        })
        .count()
}

/// Attempts `attempts` natural spawns and returns the new mobs; the caller inserts them.
///
/// With no players there is nothing to spawn around, and a full creature mob cap stops the
/// attempts before any block lookups.
pub fn natural_spawn(
    store: &EntityStore,
    env: &impl SpawnEnv,
    players: &[Vec3],
    rng: &mut Rng,
    attempts: usize,
) -> Vec<EntityRecord> {
    natural_spawn_in_chunks(
        store,
        env,
        players,
        rng,
        attempts,
        players.len().saturating_mul(SPAWN_CHUNK_AREA),
    )
}

/// [`natural_spawn`] with the number of chunks that can accept natural spawns. The region
/// passes its loaded chunk count; the compatibility wrapper above keeps the old per-player
/// cap for callers that have no chunk inventory.
pub fn natural_spawn_in_chunks(
    store: &EntityStore,
    env: &impl SpawnEnv,
    players: &[Vec3],
    rng: &mut Rng,
    attempts: usize,
    spawnable_chunks: usize,
) -> Vec<EntityRecord> {
    if players.is_empty() {
        return Vec::new();
    }
    let cap = category_cap(CREATURE_CAP_PER_PLAYER, spawnable_chunks);
    if creature_count(store) >= cap {
        return Vec::new();
    }
    let mut out = Vec::new();
    for _ in 0..attempts {
        if creature_count(store) + out.len() >= cap {
            break;
        }
        let player = &players[rng.next_int(players.len() as i32) as usize];
        let angle = rng.next_f64() * std::f64::consts::TAU;
        let distance = rng.range(MIN_SPAWN_DISTANCE, MAX_SPAWN_DISTANCE);
        let x = (player.x + angle.cos() * distance).floor() as i32;
        let z = (player.z + angle.sin() * distance).floor() as i32;
        let Some(y) = surface(env, x, z) else {
            continue;
        };
        if !can_spawn(env, x, y, z) || !far_enough(players, x, z) {
            continue;
        }
        let (kind, herd) = match env.creature_table(x, z) {
            Some(table) => {
                let Some(entry) = pick(table, rng) else {
                    continue;
                };
                (
                    entry.kind,
                    entry.min + rng.next_int(entry.max - entry.min + 1),
                )
            }
            None => (
                MobKind::ALL[rng.next_int(MobKind::ALL.len() as i32) as usize],
                1 + rng.next_int(3),
            ),
        };
        // Vanilla herds: a few animals of one kind, spread over the neighbouring blocks.
        for _ in 0..herd {
            let ox = x + rng.next_int(3) - 1;
            let oz = z + rng.next_int(3) - 1;
            let oy = surface(env, ox, oz).unwrap_or(y);
            if !can_spawn(env, ox, oy, oz) || !far_enough(players, ox, oz) {
                continue;
            }
            let pos = Vec3::new(f64::from(ox) + 0.5, f64::from(oy), f64::from(oz) + 0.5);
            out.push(new_mob(kind, pos, rng));
            if creature_count(store) + out.len() >= cap {
                break;
            }
        }
    }
    out
}

/// Number of live hostile mobs in a store.
pub fn monster_count(store: &EntityStore) -> usize {
    store
        .keys()
        .iter()
        .filter(|key| {
            store
                .data(**key)
                .is_some_and(|data| data.mob.is_some_and(|mob| mob.kind.is_hostile()))
                && store
                    .flags(**key)
                    .is_some_and(|flags| !flags.has(Flags::REMOVED))
        })
        .count()
}

/// Attempts `attempts` monster spawns and returns the new mobs; the caller inserts them.
///
/// Follows vanilla `NaturalSpawner` for the monster category: a dark floor at least 24 blocks
/// from every player and within 128 of one, in a pack of one to four, while the monster cap of
/// seventy per player is not reached. The search walks down from the surface into caves.
pub fn natural_spawn_monsters(
    store: &EntityStore,
    env: &impl SpawnEnv,
    players: &[Vec3],
    rng: &mut Rng,
    attempts: usize,
) -> Vec<EntityRecord> {
    natural_spawn_monsters_in_chunks(
        store,
        env,
        players,
        rng,
        attempts,
        players.len().saturating_mul(SPAWN_CHUNK_AREA),
    )
}

/// [`natural_spawn_monsters`] with the number of chunks that can accept natural spawns.
pub fn natural_spawn_monsters_in_chunks(
    store: &EntityStore,
    env: &impl SpawnEnv,
    players: &[Vec3],
    rng: &mut Rng,
    attempts: usize,
    spawnable_chunks: usize,
) -> Vec<EntityRecord> {
    if players.is_empty() {
        return Vec::new();
    }
    let cap = category_cap(MONSTER_CAP_PER_PLAYER, spawnable_chunks);
    if monster_count(store) >= cap {
        return Vec::new();
    }
    let mut out = Vec::new();
    for _ in 0..attempts {
        if monster_count(store) + out.len() >= cap {
            break;
        }
        let player = &players[rng.next_int(players.len() as i32) as usize];
        let angle = rng.next_f64() * std::f64::consts::TAU;
        let distance = rng.range(MIN_SPAWN_DISTANCE, MAX_SPAWN_DISTANCE);
        let x = (player.x + angle.cos() * distance).floor() as i32;
        let z = (player.z + angle.sin() * distance).floor() as i32;
        let Some(surface) = surface(env, x, z) else {
            continue;
        };
        let mut floor = None;
        for y in (surface - MONSTER_DEPTH..=surface).rev() {
            if can_spawn_monster(env, x, y, z) && far_enough(players, x, z) {
                floor = Some(y);
                break;
            }
        }
        let Some(y) = floor else {
            continue;
        };
        let (kind, pack) = match env.monster_table(x, z) {
            Some(table) => {
                let Some(entry) = pick(table, rng) else {
                    continue;
                };
                (
                    entry.kind,
                    entry.min + rng.next_int(entry.max - entry.min + 1),
                )
            }
            None => (
                MobKind::HOSTILE[rng.next_int(MobKind::HOSTILE.len() as i32) as usize],
                1 + rng.next_int(4),
            ),
        };
        // Vanilla `Slime.checkSlimeSpawnRules`: slimes only spawn deep underground (`y < 40`) in
        // slime chunks (10% of chunks) — never on sunlit beaches or plains surfaces.
        if kind == MobKind::Slime
            && (y >= 40 || !is_slime_chunk(x.div_euclid(16), z.div_euclid(16)))
        {
            continue;
        }
        // Vanilla monster packs: a few of the same kind around the spawn block.
        for _ in 0..pack {
            let ox = x + rng.next_int(3) - 1;
            let oz = z + rng.next_int(3) - 1;
            if !can_spawn_monster(env, ox, y, oz) || !far_enough(players, ox, oz) {
                continue;
            }
            let pos = Vec3::new(f64::from(ox) + 0.5, f64::from(y), f64::from(oz) + 0.5);
            out.push(new_mob(kind, pos, rng));
            if monster_count(store) + out.len() >= cap {
                break;
            }
        }
    }
    out
}

/// Whether a block can carry a monster: a solid floor, two passable blocks above and a light
/// level at or below seven (vanilla `Monster.isDarkEnoughToSpawn`).
pub fn can_spawn_monster(env: &impl SpawnEnv, x: i32, y: i32, z: i32) -> bool {
    let (Some(below), Some(feet), Some(head)) = (
        env.block_state(x, y - 1, z),
        env.block_state(x, y, z),
        env.block_state(x, y + 1, z),
    ) else {
        return false;
    };
    is_solid(below)
        && !is_leaves(below)
        && is_passable(feet)
        && is_passable(head)
        && env.light(x, y, z) <= MONSTER_MAX_LIGHT
}

fn is_leaves(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| name.ends_with("_leaves"))
}

/// Deterministic 1-in-10 slime chunk check matching vanilla's 10% slime-chunk density.
fn is_slime_chunk(cx: i32, cz: i32) -> bool {
    let h = (i64::from(cx).wrapping_mul(0x4C19_0601))
        ^ (i64::from(cz).wrapping_mul(0x5AC0_DB03))
        ^ 0x3AD8_025F;
    (h.unsigned_abs() % 10) == 0
}

pub(crate) fn is_solid(state: u16) -> bool {
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return false;
    }
    let props = ferrite_data::block_props::get(state);
    !props.is_air
        && props.fluid.is_none()
        && !ferrite_data::block_props::collision_shape(state).is_empty()
}

fn is_passable(state: u16) -> bool {
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return false;
    }
    let props = ferrite_data::block_props::get(state);
    props.is_air
        || props.fluid.is_none() && ferrite_data::block_props::collision_shape(state).is_empty()
}

/// Vanilla `PatrolSpawner` minimum offset from the chosen player, per horizontal axis.
pub const PATROL_MIN_OFFSET: i32 = 24;
/// Vanilla picks `24 + nextInt(24)` on each axis (48 is excluded).
pub const PATROL_OFFSET_SPAN: i32 = 24;
/// The smallest patrol this server forms (the task's parity target; vanilla Java uses
/// `1..=8` with `localDifficulty` extra members).
pub const PATROL_GROUP_MIN: i32 = 2;
/// The largest patrol this server forms.
pub const PATROL_GROUP_MAX: i32 = 5;
/// Vanilla patrols start only after the world age reaches 100 minutes (five in-game days).
pub const PATROL_MIN_WORLD_AGE: i64 = 120_000;
/// Vanilla `PatrolSpawner` delay between attempts, in ticks (10–11 minutes).
pub const PATROL_INTERVAL: i64 = 12_000;
/// Extra random ticks added to [`PATROL_INTERVAL`] (13200 ticks excluded).
pub const PATROL_INTERVAL_JITTER: i32 = 1_200;
/// Patrols need a *block* light level at or below eight; sky light alone does not stop them.
pub const PATROL_MAX_LIGHT: u8 = 8;
/// The largest patrol local difficulty may grow a group to (vanilla's `localDifficulty` extra
/// members on top of the 2–5 base group).
pub const PATROL_GROUP_MAX_LOCAL: i32 = PATROL_GROUP_MAX + 3;

/// Vanilla `DifficultyInstance`: the base game difficulty scaled by how long the world has
/// existed, how long the chunk has been inhabited and, on Hard, the moon phase.
///
/// The world-age term starts after 72,000 ticks (three in-game days) and fills by 1,440,000
/// ticks; the regional term fills after 3,600,000 inhabited ticks. Peaceful is always zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalDifficulty {
    base: i32,
    value: f64,
}

impl LocalDifficulty {
    /// The vanilla `DifficultyInstance` effective difficulty for one position.
    pub fn new(base: i32, total_time: i64, inhabited_time: i64, moon_brightness: f64) -> Self {
        let base = base.clamp(0, 3);
        if base == 0 {
            return Self { base, value: 0.0 };
        }
        let hard = base == 3;
        let world_age = (((total_time - 72_000) as f64) / 1_440_000.0).clamp(0.0, 1.0);
        let regional = (inhabited_time.max(0) as f64 / 3_600_000.0).clamp(0.0, 1.0);
        let mut value = 0.75 + world_age * 0.25 + regional * if hard { 1.0 } else { 0.75 };
        if hard {
            value += moon_brightness.clamp(0.0, 1.0) * 0.2;
        }
        Self { base, value }
    }

    /// The local difficulty a caller without chunk data can use: the old world-difficulty-only
    /// behaviour, kept for the compatibility wrappers and their tests.
    pub fn from_world_difficulty(base: i32) -> Self {
        let base = base.clamp(0, 3);
        Self {
            base,
            value: if base == 0 {
                0.0
            } else {
                0.75 + f64::from(base - 2).max(0.0) * 0.25
            },
        }
    }

    /// Vanilla `DifficultyInstance#getEffectiveDifficulty`.
    pub fn value(self) -> f64 {
        self.value
    }

    /// The base difficulty, used where vanilla sizes a group from `Difficulty` itself.
    pub fn difficulty(self) -> i32 {
        self.base
    }

    /// Whole extra patrol members the regional portion above the 0.75 baseline has earned.
    pub fn extra_patrol_members(self) -> i32 {
        (((self.value - 0.75) * 4.0).floor() as i32).clamp(0, 3)
    }
}

/// A patrol candidate and the local difficulty around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatrolPlayer {
    pub pos: Vec3,
    pub local_difficulty: LocalDifficulty,
}

/// Per-dimension `PatrolSpawner` timing, shared by every region of the dimension so a split
/// or merged region cannot run its own copy of the 12000-tick delay.
///
/// Every region calls [`Self::roll`] each tick with the same world clock; only the region that
/// observes the due tick advances the window, and the 1-in-5 vanilla chance decides whether
/// that region spawns a patrol.
#[derive(Debug, Default)]
pub struct PatrolSpawnerState {
    next_at: Option<i64>,
}

impl PatrolSpawnerState {
    /// Whether this call is the due patrol attempt. The first call only schedules the window.
    pub fn roll(&mut self, enabled: bool, clock_time: i64, rng: &mut Rng) -> bool {
        if !enabled || clock_time <= 0 {
            return false;
        }
        if self.next_at.is_some_and(|next| clock_time < next) {
            return false;
        }
        self.next_at =
            Some(clock_time + PATROL_INTERVAL + i64::from(rng.next_int(PATROL_INTERVAL_JITTER)));
        // Vanilla `PatrolSpawner`: a due attempt only has a 20% chance to spawn.
        rng.next_int(5) == 0
    }
}

/// A pillager patrol around a random player: a 2–5 pillager group whose first member is the
/// banner-carrying captain.
///
/// Vanilla picks `24 + nextInt(24)` blocks along both the X and Z axes from the player, places
/// the captain at the top motion-blocking block (block light at most eight), then places the
/// other members within a four-block square of the previous pillager. The caller checks the
/// dimension, difficulty and game rules. This convenience wrapper uses Normal difficulty; the
/// region calls [`spawn_patrol_with_local_difficulty`] with each player's chunk difficulty.
pub fn spawn_patrol(env: &impl SpawnEnv, players: &[Vec3], rng: &mut Rng) -> Vec<EntityRecord> {
    spawn_patrol_with_difficulty(env, players, 2, rng)
}

/// [`spawn_patrol`] with the region's per-player local difficulty. The chosen player's regional
/// difficulty adds whole extra members on top of the 2–5 base group, up to
/// [`PATROL_GROUP_MAX_LOCAL`].
pub fn spawn_patrol_with_local_difficulty(
    env: &impl SpawnEnv,
    players: &[PatrolPlayer],
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    if players.is_empty() {
        return Vec::new();
    }
    let index = rng.next_int(players.len() as i32) as usize;
    let extra = players[index].local_difficulty.extra_patrol_members();
    let positions: Vec<_> = players.iter().map(|p| p.pos).collect();
    spawn_patrol_group(env, &positions, index, extra, 1.0, rng)
}

/// [`spawn_patrol`] with local-difficulty group sizing: above Easy, one extra pillager joins
/// with a chance that rises with the difficulty (vanilla's `localDifficulty` extra members).
pub fn spawn_patrol_with_difficulty(
    env: &impl SpawnEnv,
    players: &[Vec3],
    difficulty: i32,
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    // Above Easy one extra pillager rolls with a chance that rises with the difficulty; this
    // preserves the world-difficulty-only behaviour of the compatibility wrapper.
    if players.is_empty() {
        return Vec::new();
    }
    let index = rng.next_int(players.len() as i32) as usize;
    let extra = i32::from(difficulty.clamp(0, 3) > 1);
    spawn_patrol_group(
        env,
        players,
        index,
        extra,
        f64::from(difficulty.clamp(0, 3)) / 3.0,
        rng,
    )
}

fn spawn_patrol_group(
    env: &impl SpawnEnv,
    players: &[Vec3],
    player_index: usize,
    extra: i32,
    extra_chance: f64,
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    let player = players[player_index];
    let sign_x = if rng.next_int(2) == 0 { -1 } else { 1 };
    let sign_z = if rng.next_int(2) == 0 { -1 } else { 1 };
    let x =
        player.x.floor() as i32 + sign_x * (PATROL_MIN_OFFSET + rng.next_int(PATROL_OFFSET_SPAN));
    let z =
        player.z.floor() as i32 + sign_z * (PATROL_MIN_OFFSET + rng.next_int(PATROL_OFFSET_SPAN));
    let Some(y) = surface(env, x, z) else {
        return Vec::new();
    };
    if !can_spawn_patrol(env, x, y, z) || !far_enough(players, x, z) {
        return Vec::new();
    }
    let mut count = PATROL_GROUP_MIN + rng.next_int(PATROL_GROUP_MAX - PATROL_GROUP_MIN + 1);
    for _ in 0..extra {
        if rng.next_f64() < extra_chance {
            count += 1;
        }
    }
    count = count.min(PATROL_GROUP_MAX_LOCAL);
    let mut out = Vec::new();
    let mut captain = None;
    let mut previous = (x, z);
    for index in 0..count {
        // The captain stands on the chosen block; the members form around the previous one.
        let (ox, oz, oy) = if index == 0 {
            (x, z, y)
        } else {
            let ox = previous.0 + rng.next_int(9) - 4;
            let oz = previous.1 + rng.next_int(9) - 4;
            (ox, oz, surface(env, ox, oz).unwrap_or(y))
        };
        if !can_spawn_patrol(env, ox, oy, oz) {
            continue;
        }
        previous = (ox, oz);
        let mut record = new_mob(
            MobKind::Pillager,
            Vec3::new(f64::from(ox) + 0.5, f64::from(oy), f64::from(oz) + 0.5),
            rng,
        );
        // The first member that actually found a standable column is the captain, so a failed
        // index-0 spawn cannot leave the patrol leaderless. It drops Bad Omen like a raid
        // captain (`Raider.die`).
        let leads = captain.is_none();
        if let Some(state) = record.data.mob.as_mut() {
            state.raid_captain = leads;
            state.patrol_leader = captain;
        }
        if leads && let Some(banner) = ferrite_item::ItemStack::from_name("minecraft:white_banner")
        {
            record.data.equipment.set(EquipmentSlot::Head, Some(banner));
        }
        if leads {
            captain = Some(record.data.uuid);
        }
        mob::sync_state_metadata(&mut record.data);
        out.push(record);
    }
    out
}

/// Whether a pillager can stand here: a solid floor, two passable blocks above and a low
/// block light level (patrols do not need sky access and may stand under leaves).
pub fn can_spawn_patrol(env: &impl SpawnEnv, x: i32, y: i32, z: i32) -> bool {
    let (Some(below), Some(feet), Some(head)) = (
        env.block_state(x, y - 1, z),
        env.block_state(x, y, z),
        env.block_state(x, y + 1, z),
    ) else {
        return false;
    };
    is_solid(below)
        && is_passable(feet)
        && is_passable(head)
        && env.block_light(x, y, z) <= PATROL_MAX_LIGHT
}

/// Vanilla `PhantomSpawner`: the `TIME_SINCE_REST` statistic at which insomnia begins
/// (72000 ticks, three in-game days).
pub const PHANTOM_MIN_REST_TICKS: i64 = 72_000;
/// Lowest spawn height above the player.
pub const PHANTOM_MIN_HEIGHT: i32 = 20;
/// Highest spawn height above the player.
pub const PHANTOM_MAX_HEIGHT: i32 = 34;
/// Largest horizontal Chebyshev offset from the player.
pub const PHANTOM_SPREAD: i32 = 10;
/// The largest phantom group, on Hard (vanilla uses 1–2 on Easy, 1–3 on Normal and 1–4 on
/// Hard).
pub const PHANTOM_GROUP_MAX: i32 = 4;
/// Vanilla phantoms try to spawn every 1–2 minutes.
pub const PHANTOM_ATTEMPT_MIN: u32 = 1_200;
pub const PHANTOM_ATTEMPT_JITTER: i32 = 1_200;

/// One player the phantom spawner may target this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhantomPlayer {
    pub pos: Vec3,
    /// The player's `TIME_SINCE_REST` statistic.
    pub ticks_since_rest: i64,
}

/// Vanilla `(x - 72000) / x`: the chance of a successful phantom attempt for a player who has
/// not rested for `x` ticks.
pub fn phantom_spawn_chance(ticks_since_rest: i64) -> f64 {
    if ticks_since_rest < PHANTOM_MIN_REST_TICKS {
        return 0.0;
    }
    let x = ticks_since_rest as f64;
    (x - PHANTOM_MIN_REST_TICKS as f64) / x
}

/// The phantom group a successful attempt spawns above each insomniac player. This convenience
/// wrapper uses Normal difficulty; the region calls [`spawn_phantoms_with_difficulty`].
///
/// Phantoms appear 20–34 blocks above the player, offset by at most ten blocks on each
/// horizontal axis, only where the sky is clear. The caller checks dimension, time of day and
/// the `spawn_phantoms` rule.
pub fn spawn_phantoms(
    env: &impl SpawnEnv,
    players: &[PhantomPlayer],
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    spawn_phantoms_with_difficulty(env, players, 2, rng)
}

/// [`spawn_phantoms`] with the vanilla per-difficulty group size: 1–2 on Easy, 1–3 on Normal
/// and 1–4 on Hard.
pub fn spawn_phantoms_with_difficulty(
    env: &impl SpawnEnv,
    players: &[PhantomPlayer],
    difficulty: i32,
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    let mut out = Vec::new();
    let max = difficulty.clamp(1, PHANTOM_GROUP_MAX);
    for player in players {
        spawn_phantom_group(env, player, max, rng, &mut out);
    }
    out
}

/// [`spawn_phantoms_with_difficulty`] with each player's local difficulty. Phantoms follow the
/// vanilla `Difficulty` group size (1–4); the local value is what the region actually knows.
pub fn spawn_phantoms_with_local_difficulty(
    env: &impl SpawnEnv,
    players: &[(PhantomPlayer, LocalDifficulty)],
    rng: &mut Rng,
) -> Vec<EntityRecord> {
    let mut out = Vec::new();
    for (player, local) in players {
        let max = local.difficulty().clamp(1, PHANTOM_GROUP_MAX);
        spawn_phantom_group(env, player, max, rng, &mut out);
    }
    out
}

fn spawn_phantom_group(
    env: &impl SpawnEnv,
    player: &PhantomPlayer,
    max: i32,
    rng: &mut Rng,
    out: &mut Vec<EntityRecord>,
) {
    if player.ticks_since_rest < PHANTOM_MIN_REST_TICKS {
        return;
    }
    let count = 1 + rng.next_int(max + 1);
    for _ in 0..count {
        let x = player.pos.x.floor() as i32 + rng.next_int(PHANTOM_SPREAD * 2 + 1) - PHANTOM_SPREAD;
        let z = player.pos.z.floor() as i32 + rng.next_int(PHANTOM_SPREAD * 2 + 1) - PHANTOM_SPREAD;
        let y = player.pos.y.floor() as i32
            + PHANTOM_MIN_HEIGHT
            + rng.next_int(PHANTOM_MAX_HEIGHT - PHANTOM_MIN_HEIGHT + 1);
        if !is_passable(env.block_state(x, y, z).unwrap_or(u16::MAX)) || !env.can_see_sky(x, y, z) {
            continue;
        }
        out.push(new_mob(
            MobKind::Phantom,
            Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5),
            rng,
        ));
    }
}

/// Whether a spawn column is at least 24 blocks from every player and within 128 of one.
fn far_enough(players: &[Vec3], x: i32, z: i32) -> bool {
    let cx = f64::from(x) + 0.5;
    let cz = f64::from(z) + 0.5;
    let mut too_close = false;
    let mut in_range = false;
    for player in players {
        let dx = player.x - cx;
        let dz = player.z - cz;
        let distance = dx * dx + dz * dz;
        too_close |= distance < MIN_SPAWN_DISTANCE * MIN_SPAWN_DISTANCE;
        in_range |= distance <= MAX_SPAWN_DISTANCE * MAX_SPAWN_DISTANCE;
    }
    !too_close && in_range
}

/// The terrain height natural spawning would use: one above the first non-air block below
/// [`MAX_SURFACE_Y`], or `None` for an unloaded or empty column.
pub fn surface(env: &impl SpawnEnv, x: i32, z: i32) -> Option<i32> {
    for y in (MIN_SURFACE_Y..=MAX_SURFACE_Y).rev() {
        let state = env.block_state(x, y, z)?;
        if !is_air(state) {
            return Some(y + 1);
        }
    }
    None
}

fn is_air(state: u16) -> bool {
    usize::from(state) < ferrite_data::blocks::STATE_COUNT
        && ferrite_data::block_props::get(state).is_air
}

fn is_grass(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| name == "minecraft:grass_block")
}

/// Vanilla `Animal.checkAnimalSpawnRules` and `isSpawnablePosition`: grass below, two air
/// blocks above and light above eight.
pub fn can_spawn(env: &impl SpawnEnv, x: i32, y: i32, z: i32) -> bool {
    let (Some(below), Some(feet), Some(head)) = (
        env.block_state(x, y - 1, z),
        env.block_state(x, y, z),
        env.block_state(x, y + 1, z),
    ) else {
        return false;
    };
    is_grass(below) && is_air(feet) && is_air(head) && env.light(x, y, z) >= MIN_LIGHT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::BlockGetter;

    struct Env {
        light: u8,
        floor: &'static str,
    }

    impl BlockGetter for Env {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            let name = if y == 63 {
                self.floor
            } else if y < 63 {
                "stone"
            } else {
                "air"
            };
            Some(ferrite_data::blocks::default_state(name).unwrap())
        }
    }

    impl SpawnEnv for Env {
        fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
            self.light
        }
    }

    /// A column under an open sky at noon: full combined light, no block light. Patrols check
    /// block light only, so this still accepts one.
    struct Sunlit(Env);

    impl BlockGetter for Sunlit {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            self.0.block_state(x, y, z)
        }
    }

    impl SpawnEnv for Sunlit {
        fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
            15
        }

        fn block_light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
            0
        }
    }

    fn player() -> Vec3 {
        Vec3::new(0.5, 64.0, 0.5)
    }

    struct Tabled(Env, Vec<SpawnEntry>);

    impl BlockGetter for Tabled {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            self.0.block_state(x, y, z)
        }
    }

    impl SpawnEnv for Tabled {
        fn light(&self, x: i32, y: i32, z: i32) -> u8 {
            self.0.light(x, y, z)
        }
        fn creature_table(&self, _x: i32, _z: i32) -> Option<&[SpawnEntry]> {
            Some(&self.1)
        }
    }

    #[test]
    fn spawn_cap_scales_with_spawnable_chunks() {
        assert_eq!(super::category_cap(70, 289), 70);
        assert_eq!(super::category_cap(70, 441), 106);
        assert_eq!(super::category_cap(10, 441), 15);
        assert_eq!(super::category_cap(70, 4), 0);
    }

    #[test]
    fn a_biome_table_decides_the_kinds_and_herd_sizes() {
        let mut rng = Rng::new(5);
        let store = EntityStore::default();
        let wolves = SpawnEntry {
            kind: MobKind::Wolf,
            weight: 8,
            min: 4,
            max: 4,
        };
        let env = Tabled(
            Env {
                floor: "grass_block",
                light: 15,
            },
            vec![wolves],
        );
        let spawned = natural_spawn(&store, &env, &[player()], &mut rng, 64);
        assert!(!spawned.is_empty());
        assert!(
            spawned
                .iter()
                .all(|r| r.data.mob.is_some_and(|m| m.kind == MobKind::Wolf))
        );
        // A biome with no spawns spawns nothing.
        let empty = Tabled(
            Env {
                floor: "grass_block",
                light: 15,
            },
            Vec::new(),
        );
        assert!(natural_spawn(&store, &empty, &[player()], &mut rng, 64).is_empty());
    }

    #[test]
    fn spawns_grass_animals_in_daylight_within_the_cap() {
        let env = Env {
            light: 15,
            floor: "grass_block",
        };
        let store = EntityStore::default();
        let mut rng = Rng::new(11);
        let spawned = natural_spawn(&store, &env, &[player()], &mut rng, 64);
        assert!(!spawned.is_empty(), "a daylight meadow has animals");
        for record in &spawned {
            assert_eq!(record.pos.y.floor(), 64.0);
            assert!(record.data.mob.is_some());
            let dx = record.pos.x - player().x;
            let dz = record.pos.z - player().z;
            let distance = (dx * dx + dz * dz).sqrt();
            assert!(
                (MIN_SPAWN_DISTANCE..=MAX_SPAWN_DISTANCE + 2.0).contains(&distance),
                "spawned {distance} blocks from the player"
            );
        }
    }

    #[test]
    fn darkness_stone_and_an_absent_player_never_spawn() {
        let mut rng = Rng::new(12);
        let store = EntityStore::default();
        let dark = Env {
            light: 7,
            floor: "grass_block",
        };
        assert!(natural_spawn(&store, &dark, &[player()], &mut rng, 64).is_empty());
        let stone = Env {
            light: 15,
            floor: "stone",
        };
        assert!(natural_spawn(&store, &stone, &[player()], &mut rng, 64).is_empty());
        assert!(natural_spawn(&store, &stone, &[], &mut rng, 64).is_empty());
    }

    #[test]
    fn the_creature_mob_cap_stops_spawning() {
        let env = Env {
            light: 15,
            floor: "grass_block",
        };
        let mut store = EntityStore::default();
        let mut rng = Rng::new(13);
        for _ in 0..CREATURE_CAP_PER_PLAYER {
            store.insert(new_mob(MobKind::Pig, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        }
        assert_eq!(creature_count(&store), CREATURE_CAP_PER_PLAYER);
        assert!(natural_spawn(&store, &env, &[player()], &mut rng, 64).is_empty());
        // A second player raises the cap, so the meadow is used again.
        let other = Vec3::new(64.5, 64.0, 64.5);
        assert!(!natural_spawn(&store, &env, &[player(), other], &mut rng, 64).is_empty());
    }

    #[test]
    fn monsters_spawn_on_dark_floors_up_to_the_monster_cap() {
        struct Cave {
            light: u8,
        }
        impl BlockGetter for Cave {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                let name = if y < 50 { "stone" } else { "air" };
                Some(ferrite_data::blocks::default_state(name).unwrap())
            }
        }
        impl SpawnEnv for Cave {
            fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
                self.light
            }
        }
        let dark = Cave { light: 3 };
        let store = EntityStore::default();
        let mut rng = Rng::new(21);
        let spawned = natural_spawn_monsters(&store, &dark, &[player()], &mut rng, 64);
        assert!(!spawned.is_empty(), "a dark cave spawns monsters");
        assert!(
            spawned
                .iter()
                .all(|record| record.data.mob.is_some_and(|mob| mob.kind.is_hostile()))
        );
        // A lit cave never spawns monsters.
        let bright = Cave { light: 15 };
        assert!(natural_spawn_monsters(&store, &bright, &[player()], &mut rng, 64).is_empty());
        // The monster cap stops spawning.
        let mut full = EntityStore::default();
        for _ in 0..MONSTER_CAP_PER_PLAYER {
            full.insert(new_mob(
                MobKind::Zombie,
                Vec3::new(0.5, 50.0, 0.5),
                &mut rng,
            ));
        }
        assert_eq!(monster_count(&full), MONSTER_CAP_PER_PLAYER);
        assert!(natural_spawn_monsters(&full, &dark, &[player()], &mut rng, 64).is_empty());
    }

    #[test]
    fn surface_finds_the_first_open_block() {
        let env = Env {
            light: 15,
            floor: "grass_block",
        };
        assert_eq!(surface(&env, 0, 0), Some(64));
        // A column with a roof above the surface still spawns on the surface below it; the
        // light rule then rejects the covered spot.
        struct Roof;
        impl BlockGetter for Roof {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                let name = match y {
                    80 => "stone",
                    y if y < 63 => "stone",
                    _ => "air",
                };
                Some(ferrite_data::blocks::default_state(name).unwrap())
            }
        }
        impl SpawnEnv for Roof {
            fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
                7
            }
        }
        assert_eq!(surface(&Roof, 0, 0), Some(81));
        assert!(!can_spawn(&Roof, 0, 81, 0));
    }

    #[test]
    fn monsters_never_spawn_on_leaves_and_slimes_never_spawn_on_surface() {
        let leaves_env = Env {
            light: 0,
            floor: "oak_leaves",
        };
        assert!(
            !can_spawn_monster(&leaves_env, 0, 64, 0),
            "hostile mobs must not spawn on tree leaves"
        );

        struct SlimeOnlyBeach;
        impl BlockGetter for SlimeOnlyBeach {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                let name = if y <= 63 { "sand" } else { "air" };
                Some(ferrite_data::blocks::default_state(name).unwrap())
            }
        }
        impl SpawnEnv for SlimeOnlyBeach {
            fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
                0
            }
            fn monster_table(&self, _x: i32, _z: i32) -> Option<&[SpawnEntry]> {
                static SLIMES: [SpawnEntry; 1] = [SpawnEntry {
                    kind: MobKind::Slime,
                    weight: 100,
                    min: 1,
                    max: 4,
                }];
                Some(&SLIMES)
            }
        }
        let store = EntityStore::default();
        let mut rng = Rng::new(42);
        let spawned = natural_spawn_monsters(&store, &SlimeOnlyBeach, &[player()], &mut rng, 128);
        assert!(
            spawned.is_empty(),
            "slimes must not spawn on surface beaches (y >= 40)"
        );

        // Hostile monsters must not consume the passive creature cap.
        let mut mixed = EntityStore::default();
        for _ in 0..(CREATURE_CAP_PER_PLAYER + 5) {
            mixed.insert(new_mob(
                MobKind::Zombie,
                Vec3::new(0.5, 50.0, 0.5),
                &mut rng,
            ));
        }
        assert_eq!(creature_count(&mixed), 0);
        assert_eq!(monster_count(&mixed), CREATURE_CAP_PER_PLAYER + 5);
    }

    #[test]
    fn a_patrol_is_two_to_five_pillagers_with_one_banner_captain() {
        let env = Env {
            light: 0,
            floor: "grass_block",
        };
        let player = Vec3::new(0.5, 64.0, 0.5);
        let mut rng = Rng::new(7);
        // The captain's chosen column must accept a spawn; retry like the vanilla attempt loop.
        let patrol = (0..64)
            .find_map(|_| {
                let patrol = spawn_patrol(&env, &[player], &mut rng);
                (!patrol.is_empty()).then_some(patrol)
            })
            .expect("a patrol eventually spawns on the surface");
        assert!(
            (PATROL_GROUP_MIN..=PATROL_GROUP_MAX).contains(&(patrol.len() as i32)),
            "patrol size {}",
            patrol.len()
        );
        let captains = patrol
            .iter()
            .filter(|record| record.data.mob.as_ref().is_some_and(|mob| mob.raid_captain))
            .count();
        assert_eq!(captains, 1, "exactly one pillager leads the patrol");
        assert!(
            patrol[0]
                .data
                .mob
                .as_ref()
                .is_some_and(|mob| mob.raid_captain)
        );
        let banner = patrol[0]
            .data
            .equipment
            .get(EquipmentSlot::Head)
            .and_then(|stack| stack.name());
        assert_eq!(banner, Some("minecraft:white_banner"));
        for record in &patrol {
            let mob = record.data.mob.as_ref().expect("a patrol member is a mob");
            assert_eq!(mob.kind, MobKind::Pillager);
            // Everyone but the captain follows the banner carrier (the first successful member).
            if mob.raid_captain {
                assert_eq!(mob.patrol_leader, None);
            } else {
                assert_eq!(mob.patrol_leader, Some(patrol[0].data.uuid));
            }
            let dx = (record.pos.x - player.x).abs();
            let dz = (record.pos.z - player.z).abs();
            assert!(
                ((PATROL_MIN_OFFSET as f64 - 5.0)..=(PATROL_MIN_OFFSET as f64 + 50.0))
                    .contains(&dx.max(dz)),
                "patrol member {dx}/{dz} blocks from the player"
            );
        }
        // A torch-lit column refuses the patrol (block light above eight).
        let lit = Env {
            light: 15,
            floor: "grass_block",
        };
        for _ in 0..64 {
            assert!(spawn_patrol(&lit, &[player], &mut rng).is_empty());
        }
        // Sunlight is not block light: the same surface under an open sky still accepts one.
        let sunlit = Sunlit(Env {
            light: 0,
            floor: "grass_block",
        });
        assert!(
            (0..64).any(|_| !spawn_patrol(&sunlit, &[player], &mut rng).is_empty()),
            "sunlight alone must not block a patrol"
        );
        assert!(spawn_patrol(&env, &[], &mut rng).is_empty());
    }

    #[test]
    fn local_difficulty_grows_patrols_with_world_age_and_inhabited_time() {
        let fresh = LocalDifficulty::new(2, 0, 0, 0.0);
        assert!((fresh.value() - 0.75).abs() < 1e-9);
        assert_eq!(fresh.extra_patrol_members(), 0);
        let aged = LocalDifficulty::new(2, 1_512_000, 3_600_000, 0.0);
        assert!((aged.value() - 1.75).abs() < 1e-9);
        assert_eq!(aged.extra_patrol_members(), 3);
        // Peaceful never scales, and the moon only matters on Hard.
        assert_eq!(
            LocalDifficulty::new(0, 9_999_999, 9_999_999, 1.0).value(),
            0.0
        );
        assert!(
            LocalDifficulty::new(3, 0, 0, 1.0).value() > LocalDifficulty::new(2, 0, 0, 1.0).value()
        );

        let env = Env {
            light: 0,
            floor: "grass_block",
        };
        let player = Vec3::new(0.5, 64.0, 0.5);
        let mut rng = Rng::new(11);
        let grown = (0..64)
            .find_map(|_| {
                let patrol = spawn_patrol_with_local_difficulty(
                    &env,
                    &[PatrolPlayer {
                        pos: player,
                        local_difficulty: aged,
                    }],
                    &mut rng,
                );
                (patrol.len() > PATROL_GROUP_MAX as usize).then_some(patrol)
            })
            .expect("a long-inhabited Hard world grows patrols past the base group");
        assert!(grown.len() as i32 <= PATROL_GROUP_MAX_LOCAL);
        assert_eq!(
            grown
                .iter()
                .filter(|record| record.data.mob.as_ref().is_some_and(|mob| mob.raid_captain))
                .count(),
            1
        );
    }

    #[test]
    fn the_patrol_timer_is_shared_so_regions_never_double_spawn() {
        let mut rng = Rng::new(3);
        let mut state = PatrolSpawnerState::default();
        // The first observation only schedules the window.
        assert!(!state.roll(true, 1_000, &mut rng));
        // Every other region of the dimension sees the same window and cannot spawn earlier.
        for clock in 1_001..12_000 {
            assert!(!state.roll(true, clock, &mut rng));
            assert!(!state.roll(true, clock, &mut rng));
        }
        // A due window never spawns twice, even when two regions tick in the same clock tick.
        let mut spawned = 0;
        for clock in 12_000..(1_000 + PATROL_INTERVAL + i64::from(PATROL_INTERVAL_JITTER) + 2) {
            let first = state.roll(true, clock, &mut rng);
            let second = state.roll(true, clock, &mut rng);
            assert!(!(first && second), "two regions spawned one patrol");
            if first || second {
                spawned += 1;
            }
        }
        assert!(spawned <= 1, "one window makes at most one patrol");
        // A disabled rule neither spawns nor advances the window.
        assert!(!state.roll(false, i64::MAX, &mut rng));
    }

    #[test]
    fn phantoms_spawn_one_to_three_on_normal_twenty_to_thirty_four_blocks_above() {
        let env = Env {
            light: 0,
            floor: "grass_block",
        };
        let player = PhantomPlayer {
            pos: Vec3::new(0.5, 64.0, 0.5),
            ticks_since_rest: PHANTOM_MIN_REST_TICKS + 72_000,
        };
        let mut rng = Rng::new(9);
        let phantoms = spawn_phantoms(&env, &[player], &mut rng);
        assert!(
            (1..=3).contains(&(phantoms.len() as i32)),
            "normal phantom group {}",
            phantoms.len()
        );
        // Easy caps the group at two, Hard at four.
        for (difficulty, max) in [(1, 2), (2, 3), (3, PHANTOM_GROUP_MAX)] {
            let group = spawn_phantoms_with_difficulty(&env, &[player], difficulty, &mut rng);
            assert!(
                (1..=max).contains(&(group.len() as i32)),
                "difficulty {difficulty} group {}",
                group.len()
            );
        }
        for record in &phantoms {
            assert!(
                record
                    .data
                    .mob
                    .as_ref()
                    .is_some_and(|mob| mob.kind == MobKind::Phantom)
            );
            let dy = record.pos.y - player.pos.y;
            assert!(
                (f64::from(PHANTOM_MIN_HEIGHT)..=f64::from(PHANTOM_MAX_HEIGHT)).contains(&dy),
                "phantom spawned {dy} blocks above"
            );
            assert!((record.pos.x - player.pos.x).abs() <= f64::from(PHANTOM_SPREAD));
            assert!((record.pos.z - player.pos.z).abs() <= f64::from(PHANTOM_SPREAD));
        }
        // A player who rested recently never spawns phantoms, and below a roof neither does one
        // whose sky is blocked.
        let rested = PhantomPlayer {
            pos: player.pos,
            ticks_since_rest: PHANTOM_MIN_REST_TICKS - 1,
        };
        assert!(spawn_phantoms(&env, &[rested], &mut rng).is_empty());
        assert_eq!(
            phantom_spawn_chance(PHANTOM_MIN_REST_TICKS - 1),
            0.0,
            "insomnia needs three in-game days without sleep"
        );
        assert_eq!(phantom_spawn_chance(PHANTOM_MIN_REST_TICKS), 0.0);
        assert!((phantom_spawn_chance(144_000) - 0.5).abs() < 1.0e-9);
    }

    #[test]
    fn phantoms_need_clear_sky_above_the_player() {
        struct Roofed;
        impl BlockGetter for Roofed {
            fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
                Some(
                    ferrite_data::blocks::default_state(if y <= 63 { "stone" } else { "air" })
                        .unwrap(),
                )
            }
        }
        impl SpawnEnv for Roofed {
            fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
                0
            }
            fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> bool {
                false
            }
        }
        let player = PhantomPlayer {
            pos: Vec3::new(0.5, 64.0, 0.5),
            ticks_since_rest: PHANTOM_MIN_REST_TICKS,
        };
        let mut rng = Rng::new(11);
        assert!(spawn_phantoms(&Roofed, &[player], &mut rng).is_empty());
    }
}
