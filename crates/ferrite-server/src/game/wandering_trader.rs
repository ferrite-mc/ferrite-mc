//! Wandering-trader spawning and the invisibility/milk cycle.
//!
//! Vanilla 26.3 rolls the trader spawner every 1200 ticks: every 20th roll (24000 ticks) it
//! makes ten attempts around a random player. The chance starts at 25%, grows by 25% on a
//! failed daily roll and resets to 25% after a spawn. An attempt picks a column within
//! `-48..47` blocks, searches from `playerY + 16` down 48 blocks for `MOTION_BLOCKING_NO_LEAVES`
//! ground with room for the trader, and rejects the `without_wandering_trader_spawns` biome
//! tag.
//!
//! The 26.3 trader does not despawn with a potion. At night it drinks an invisibility potion
//! and by day it drinks milk, which removes the invisibility; the entity and its offers stay
//! in place. Its separate 48000-tick despawn delay is handled by the region trader lifecycle,
//! which pauses while a player has its trade screen open.

use ferrite_entity::{
    BlockGetter, EntityData, Rng,
    effect::{Effect, MobEffectInstance},
};

/// Number of candidate columns the vanilla spawner tries per daily roll.
pub const SPAWN_ATTEMPTS: i32 = 10;
/// Horizontal radius of the vanilla candidate search (`-48..47`).
pub const SPAWN_RADIUS: i32 = 48;
/// The first search row is this many blocks above the selected player.
pub const SEARCH_ABOVE: i32 = 16;
/// The search goes this many blocks below the first row (inclusive).
pub const SEARCH_DEPTH: i32 = 48;
/// Vanilla `WanderingTraderSpawner.SPAWN_ONE_IN_X_CHANCE`.
pub const SPAWN_ONE_IN_X_CHANCE: i32 = 10;
/// The spawn chance after a successful spawn.
pub const MIN_SPAWN_CHANCE: u32 = 25;
/// The maximum daily spawn chance.
pub const MAX_SPAWN_CHANCE: u32 = 75;
/// Vanilla trader despawn delay, kept for the lifecycle tests and the spawner caller.
pub const DESPAWN_DELAY: u32 = 48_000;
/// Vanilla invisibility potion duration used by the trader's night goal.
pub const INVISIBILITY_TICKS: i32 = 3600;
/// The day-time range in which `isDarkOutside` is true (26.3, ticks 13000..22999).
pub const NIGHT_START: i64 = 13_000;
pub const NIGHT_END: i64 = 23_000;

/// Per-dimension state for the wandering trader spawner. It is shared by every region in a
/// dimension so concurrent region ticks cannot each perform the same daily roll.
#[derive(Debug, Default)]
pub struct SpawnerState {
    chance: u32,
    last_roll_at: Option<i64>,
}

impl SpawnerState {
    /// Performs at most one daily roll for a given world clock value.
    pub fn roll(&mut self, enabled: bool, tick_clock: i64, rng: &mut Rng) -> bool {
        if !enabled
            || tick_clock <= 0
            || tick_clock % 24_000 != 0
            || self.last_roll_at == Some(tick_clock)
        {
            return false;
        }
        self.last_roll_at = Some(tick_clock);
        roll_spawn(enabled, tick_clock, &mut self.chance, rng)
    }

    /// Resets the chance after a spawn only if it belongs to the roll that authorized it.
    pub fn spawn_succeeded(&mut self, tick_clock: i64) {
        if self.last_roll_at == Some(tick_clock) {
            spawn_succeeded(&mut self.chance);
        }
    }
}

/// Lower bound for the spawn column's light. The vanilla spawner only reads the heightmap and
/// has no light predicate; this keeps cave floors found by the wider column scan out while
/// preserving normal surface spawns (stored sky light is 15 at the surface at any time).
pub const MIN_SPAWN_LIGHT: u8 = 9;

/// World queries the spawner needs. `TickBlocks` implements this in the region.
pub trait SpawnWorld: BlockGetter {
    /// Combined block/sky light at a block position.
    fn light(&self, x: i32, y: i32, z: i32) -> u8;
    /// Namespaced biome key at a block position.
    fn biome_name(&self, x: i32, y: i32, z: i32) -> Option<String>;
    /// Whether the chunk containing `(x, z)` is already loaded. The spawner must never ask the
    /// world to generate a chunk just to place a trader.
    fn is_chunk_loaded(&self, _x: i32, _z: i32) -> bool {
        true
    }
}

/// The border and build-height limits for one spawn search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnBounds {
    pub min_y: i32,
    /// Exclusive top of the dimension.
    pub max_y: i32,
    pub border_center_x: f64,
    pub border_center_z: f64,
    pub border_diameter: f64,
}

impl SpawnBounds {
    /// The overworld limits, with the vanilla default world border. Used by focused tests.
    #[cfg(test)]
    pub const OVERWORLD: Self = Self {
        min_y: -64,
        max_y: 320,
        border_center_x: 0.0,
        border_center_z: 0.0,
        border_diameter: 59_999_968.0,
    };

    /// Whether a block column lies inside the world border. Matches the border check used by
    /// piston movement: the minimum is inclusive and the maximum is exclusive.
    pub fn contains_column(self, x: i32, z: i32) -> bool {
        const LIMIT: f64 = 29_999_984.0;
        let half = self.border_diameter / 2.0;
        let x = f64::from(x);
        let z = f64::from(z);
        x >= (self.border_center_x - half).clamp(-LIMIT, LIMIT)
            && x < (self.border_center_x + half).clamp(-LIMIT, LIMIT)
            && z >= (self.border_center_z - half).clamp(-LIMIT, LIMIT)
            && z < (self.border_center_z + half).clamp(-LIMIT, LIMIT)
    }

    /// Whether a two-block-tall entity can stand at feet level `y`.
    pub fn fits_two_blocks(self, y: i32) -> bool {
        y >= self.min_y && y + 1 < self.max_y
    }
}

/// A daily spawn roll, including the vanilla chance growth and reset rules.
///
/// The region checks the gamerule and that a player exists before calling this. A failed daily
/// roll (either the percentage roll or the one-in-ten roll) still increments the chance; a
/// successful spawn resets it with [`spawn_succeeded`].
pub fn roll_spawn(enabled: bool, tick_clock: i64, chance: &mut u32, rng: &mut Rng) -> bool {
    if !enabled || tick_clock <= 0 || tick_clock % 24_000 != 0 {
        return false;
    }
    let current = (*chance).max(MIN_SPAWN_CHANCE);
    let passed_percentage = (rng.next_int(100) as u32) < current;
    *chance = (current + 25).min(MAX_SPAWN_CHANCE);
    if !passed_percentage {
        return false;
    }
    rng.next_int(SPAWN_ONE_IN_X_CHANCE) == 0
}

/// Resets the daily chance after a trader was actually spawned.
pub fn spawn_succeeded(chance: &mut u32) {
    *chance = MIN_SPAWN_CHANCE;
}

/// Advances a trader or trader llama despawn clock by one tick.
///
/// Vanilla only decrements `WanderingTrader.despawnDelay` while no player is trading with the
/// trader, so an open trade screen pauses the clock at its current value. A zero clock means
/// "not managed here" (for example an entity loaded from an older save) and is left alone.
/// Returns whether the clock just reached zero and the entity should disappear this tick.
pub fn tick_despawn_delay(lifetime: &mut u32, trading: bool) -> bool {
    if trading || *lifetime == 0 {
        return false;
    }
    *lifetime -= 1;
    *lifetime == 0
}

/// Finds a trader position near `anchor`, up to 48 blocks horizontally and from 16 above to 32
/// below the anchor (the 48-block search depth is inclusive). Candidates outside the world
/// border or build height, or in a chunk that is not loaded, are skipped without generating
/// anything.
pub fn find_spawn<W: SpawnWorld + ?Sized>(
    world: &W,
    anchor: [f64; 3],
    bounds: SpawnBounds,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    find_spawn_with(world, anchor, SPAWN_RADIUS, bounds, rng)
}

/// Finds a position for one trader llama within four blocks of its trader.
pub fn find_llama_spawn<W: SpawnWorld + ?Sized>(
    world: &W,
    center: [f64; 3],
    bounds: SpawnBounds,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    find_spawn_with(world, center, 4, bounds, rng).filter(|pos| {
        // A llama has a slightly wider collision box than a trader; require its second air
        // block as well.
        world
            .block_state(pos[0], pos[1] + 1, pos[2])
            .is_some_and(is_air)
            && world
                .biome_name(pos[0], pos[1], pos[2])
                .as_deref()
                .is_some_and(biome_allows)
    })
}

fn find_spawn_with<W: SpawnWorld + ?Sized>(
    world: &W,
    anchor: [f64; 3],
    radius: i32,
    bounds: SpawnBounds,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    let anchor_y = anchor[1].floor() as i32;
    let top = (anchor_y + SEARCH_ABOVE).min(bounds.max_y.saturating_sub(1));
    for _ in 0..SPAWN_ATTEMPTS {
        let x = anchor[0].floor() as i32 + rng.next_int(radius * 2) - radius;
        let z = anchor[2].floor() as i32 + rng.next_int(radius * 2) - radius;
        if !bounds.contains_column(x, z) || !world.is_chunk_loaded(x, z) {
            continue;
        }
        let Some(ground) = find_ground(world, x, z, top) else {
            continue;
        };
        let pos = [x, ground + 1, z];
        if !bounds.fits_two_blocks(pos[1]) {
            continue;
        }
        if candidate_is_valid(world, pos) {
            return Some(pos);
        }
    }
    None
}

/// The heightmap-style ground scan: from `top` down to 48 blocks below it (49 rows), the first
/// non-air block with two air blocks above it.
pub fn find_ground<W: SpawnWorld + ?Sized>(world: &W, x: i32, z: i32, top: i32) -> Option<i32> {
    for y in (top - SEARCH_DEPTH..=top).rev() {
        let state = world.block_state(x, y, z)?;
        if is_ground(state)
            && world.block_state(x, y + 1, z).is_some_and(is_air)
            && world.block_state(x, y + 2, z).is_some_and(is_air)
        {
            return Some(y);
        }
    }
    None
}

/// Validates the block environment of a candidate. The spawner's heightmap scan already
/// checked the ground; this adds light and biome filtering.
pub fn candidate_is_valid<W: SpawnWorld + ?Sized>(world: &W, pos: [i32; 3]) -> bool {
    let [x, y, z] = pos;
    if world
        .block_state(x, y - 1, z)
        .is_none_or(|state| !is_ground(state))
    {
        return false;
    }
    if !world.block_state(x, y, z).is_some_and(is_air)
        || !world.block_state(x, y + 1, z).is_some_and(is_air)
    {
        return false;
    }
    if world.light(x, y, z) < MIN_SPAWN_LIGHT {
        return false;
    }
    world
        .biome_name(x, y, z)
        .as_deref()
        .is_some_and(biome_allows)
}

/// The `without_wandering_trader_spawns` biome tag contains only `minecraft:the_void` in 26.3.
pub fn biome_allows(biome: &str) -> bool {
    !ferrite_data::tags::contains("worldgen/biome", "without_wandering_trader_spawns", biome)
}

fn is_air(state: u16) -> bool {
    ferrite_data::block_props::get(state).is_air
}

fn is_ground(state: u16) -> bool {
    if is_air(state) || ferrite_data::block_props::get(state).fluid.is_some() {
        return false;
    }
    // Exactly vanilla's `MOTION_BLOCKING_NO_LEAVES`, which excludes leaves.
    ferrite_data::block_props::get(state).heightmaps
        & ferrite_data::block_props::MOTION_BLOCKING_NO_LEAVES
        != 0
}

/// The trader's two drink goals. The night goal is an invisibility potion; the day goal is a
/// milk bucket that removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drink {
    Invisibility,
    Milk,
}

/// Chooses the next drink transition from the overworld clock and current invisibility. The
/// transition is idempotent: a trader that is already invisible at night gets no new potion,
/// and a visible trader by day gets no milk. Like both vanilla `UseItemGoal` predicates, the
/// goals stop while a player has the trading screen open.
pub fn drink_for(day_time: i64, invisible: bool, trading: bool) -> Option<Drink> {
    if trading {
        return None;
    }
    let time = day_time.rem_euclid(24_000);
    let dark = (NIGHT_START..NIGHT_END).contains(&time);
    match (dark, invisible) {
        (true, false) => Some(Drink::Invisibility),
        (false, true) => Some(Drink::Milk),
        _ => None,
    }
}

/// What one [`apply_drink`] changed, so the region can send the matching effect packets.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AppliedDrink {
    pub added: Option<MobEffectInstance>,
    pub removed: Vec<MobEffectInstance>,
}

/// Applies a trader drink to its living state without touching [`ferrite_entity::villager`]
/// offers, so trades and their uses survive every day/night transition.
pub fn apply_drink(data: &mut EntityData, drink: Drink) -> AppliedDrink {
    match drink {
        Drink::Invisibility => {
            if data.effects.has(Effect::Invisibility) {
                return AppliedDrink::default();
            }
            let instance = MobEffectInstance::new(Effect::Invisibility.id(), INVISIBILITY_TICKS, 0);
            data.effects.add(instance);
            AppliedDrink {
                added: Some(instance),
                removed: Vec::new(),
            }
        }
        Drink::Milk => {
            let (removed, _) = data.effects.clear();
            AppliedDrink {
                added: None,
                removed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::mob::{MobKind, new_mob};
    use ferrite_math::Vec3;

    const AIR: u16 = 0;

    struct TestWorld {
        floor: Option<i32>,
        ceiling: bool,
        light: u8,
        biome: &'static str,
        loaded: bool,
    }

    impl TestWorld {
        fn surface(floor: i32) -> Self {
            Self {
                floor: Some(floor),
                ceiling: false,
                light: 15,
                biome: "minecraft:plains",
                loaded: true,
            }
        }
    }

    impl BlockGetter for TestWorld {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            if self.floor == Some(y) {
                return ferrite_data::blocks::default_state("grass_block");
            }
            if self.ceiling && y == self.floor.unwrap_or(0) + 2 {
                return ferrite_data::blocks::default_state("stone");
            }
            Some(AIR)
        }
    }

    impl SpawnWorld for TestWorld {
        fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
            self.light
        }

        fn biome_name(&self, _x: i32, _y: i32, _z: i32) -> Option<String> {
            Some(self.biome.into())
        }

        fn is_chunk_loaded(&self, _x: i32, _z: i32) -> bool {
            self.loaded
        }
    }

    #[test]
    fn the_gamerule_disables_the_daily_roll_without_changing_the_chance() {
        let mut chance = 25;
        let mut rng = Rng::new(7);
        assert!(!roll_spawn(false, 24_000, &mut chance, &mut rng));
        assert_eq!(chance, 25);
    }

    #[test]
    fn turning_the_gamerule_back_on_resumes_the_next_daily_roll() {
        let mut chance = 25;
        let mut rng = Rng::new(2);
        assert!(!roll_spawn(false, 24_000, &mut chance, &mut rng));
        assert_eq!(chance, 25, "a disabled day must not advance the chance");

        // The disabled day is skipped entirely; a non-boundary tick still does nothing.
        assert!(!roll_spawn(true, 47_999, &mut chance, &mut rng));
        assert_eq!(chance, 25);

        // Seed 2 misses the percentage roll (10 < 25) and the one-in-ten roll (6), so the
        // next day grows the chance even though no trader appears.
        assert!(!roll_spawn(true, 48_000, &mut chance, &mut rng));
        assert_eq!(chance, 50, "the first enabled daily roll must run normally");

        assert!(!roll_spawn(true, 72_000, &mut chance, &mut rng));
        assert_eq!(chance, 75);
    }

    #[test]
    fn failed_rolls_grow_the_chance_and_a_successful_spawn_resets_it() {
        // Seed 2: 10 < 25 and 6 != 0, a failed one-in-ten roll.
        let mut chance = 25;
        assert!(!roll_spawn(true, 24_000, &mut chance, &mut Rng::new(2)));
        assert_eq!(chance, 50);

        // Seed 41: 13 < 25 and 0 == 0, a successful one-in-ten roll.
        let mut chance = 25;
        assert!(roll_spawn(true, 24_000, &mut chance, &mut Rng::new(41)));
        assert_eq!(chance, 50, "the chance advances before the reset");
        spawn_succeeded(&mut chance);
        assert_eq!(chance, 25);
    }

    #[test]
    fn the_daily_chance_caps_at_seventy_five() {
        let mut chance = 75;
        let _ = roll_spawn(true, 24_000, &mut chance, &mut Rng::new(2));
        assert_eq!(chance, 75);
    }

    #[test]
    fn the_daily_roll_only_happens_every_24000_ticks() {
        let mut chance = 25;
        let mut rng = Rng::new(7);
        assert!(!roll_spawn(true, 23_999, &mut chance, &mut rng));
        assert_eq!(chance, 25);
        assert!(!roll_spawn(true, 0, &mut chance, &mut rng));
        assert_eq!(chance, 25);
    }

    #[test]
    fn the_default_zero_chance_starts_at_twenty_five_percent() {
        let mut chance = 0;
        let mut rng = Rng::new(11);
        // The chance is advanced even when the percentage roll misses; it must never be read
        // as zero.
        let _ = roll_spawn(true, 24_000, &mut chance, &mut rng);
        assert!(matches!(chance, 50 | 75), "chance was {chance}");
    }

    #[test]
    fn concurrent_regions_share_one_daily_roll_and_one_chance_increment() {
        use std::sync::{Arc, Mutex};

        let state = Arc::new(Mutex::new(SpawnerState::default()));
        let threads: Vec<_> = (0..12)
            .map(|seed| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    state
                        .lock()
                        .unwrap()
                        .roll(true, 24_000, &mut Rng::new(seed))
                })
            })
            .collect();
        for thread in threads {
            let _ = thread.join().unwrap();
        }

        let state = state.lock().unwrap();
        assert_eq!(state.chance, 50, "the shared chance advances only once");
        assert_eq!(state.last_roll_at, Some(24_000));
    }

    #[test]
    fn a_successful_spawn_resets_only_its_daily_roll() {
        let mut state = SpawnerState::default();
        let _ = state.roll(true, 24_000, &mut Rng::new(2));
        state.spawn_succeeded(24_000);
        assert_eq!(state.chance, MIN_SPAWN_CHANCE);

        let _ = state.roll(true, 48_000, &mut Rng::new(2));
        state.spawn_succeeded(24_000);
        assert_eq!(state.chance, 50, "an old spawn must not reset a newer roll");
    }

    #[test]
    fn a_spawn_resets_the_chance_to_twenty_five() {
        let mut chance = 75;
        spawn_succeeded(&mut chance);
        assert_eq!(chance, 25);
    }

    #[test]
    fn the_ground_search_starts_sixteen_above_and_includes_forty_eight_below() {
        let anchor_y = 64;
        let top = anchor_y + SEARCH_ABOVE;
        let at_bottom = TestWorld::surface(top - SEARCH_DEPTH);
        assert_eq!(find_ground(&at_bottom, 0, 0, top), Some(top - SEARCH_DEPTH));

        let below_bottom = TestWorld::surface(top - SEARCH_DEPTH - 1);
        assert_eq!(find_ground(&below_bottom, 0, 0, top), None);

        let above_top = TestWorld::surface(top + 1);
        assert_eq!(find_ground(&above_top, 0, 0, top), None);
    }

    #[test]
    fn a_candidate_needs_a_light_level_and_an_allowed_biome() {
        let valid = TestWorld::surface(64);
        assert!(candidate_is_valid(&valid, [0, 65, 0]));

        let dark = TestWorld {
            light: MIN_SPAWN_LIGHT - 1,
            ..TestWorld::surface(64)
        };
        assert!(!candidate_is_valid(&dark, [0, 65, 0]));

        let void = TestWorld {
            biome: "minecraft:the_void",
            ..TestWorld::surface(64)
        };
        assert!(!candidate_is_valid(&void, [0, 65, 0]));

        let blocked = TestWorld {
            ceiling: true,
            ..TestWorld::surface(64)
        };
        assert!(!candidate_is_valid(&blocked, [0, 65, 0]));

        let no_ground = TestWorld {
            floor: None,
            ..TestWorld::surface(64)
        };
        assert!(!candidate_is_valid(&no_ground, [0, 65, 0]));
    }

    #[test]
    fn a_valid_world_produces_a_trader_inside_the_search_box() {
        let world = TestWorld::surface(64);
        let anchor = [1.5, 64.0, 2.5];
        let mut rng = Rng::new(19);
        let pos = find_spawn(&world, anchor, SpawnBounds::OVERWORLD, &mut rng)
            .expect("flat surface is valid");
        assert_eq!(pos[1], 65);
        assert!((pos[0] - 1).abs() <= SPAWN_RADIUS);
        assert!((pos[2] - 2).abs() <= SPAWN_RADIUS);
    }

    #[test]
    fn an_invalid_world_produces_no_trader_after_ten_attempts() {
        let world = TestWorld {
            biome: "minecraft:the_void",
            ..TestWorld::surface(64)
        };
        assert_eq!(
            find_spawn(
                &world,
                [0.5, 64.0, 0.5],
                SpawnBounds::OVERWORLD,
                &mut Rng::new(19)
            ),
            None
        );
    }

    #[test]
    fn candidates_outside_the_border_height_or_loaded_chunks_are_rejected() {
        let world = TestWorld::surface(64);
        let anchor = [100.5, 64.0, 100.5];

        // A ten-block border around the origin cannot contain any column in the 100 +/- 48
        // search box.
        let border = SpawnBounds {
            border_diameter: 10.0,
            ..SpawnBounds::OVERWORLD
        };
        assert_eq!(find_spawn(&world, anchor, border, &mut Rng::new(19)), None);

        // The only floor is at y = 63, so the candidate feet row would be y = 64 and the
        // exclusive top at 64 leaves no room for a two-block entity.
        let low = SpawnBounds {
            max_y: 64,
            ..SpawnBounds::OVERWORLD
        };
        assert_eq!(
            find_spawn(
                &TestWorld::surface(63),
                [0.5, 64.0, 0.5],
                low,
                &mut Rng::new(19)
            ),
            None
        );

        // An unloaded chunk must be skipped even though a fake block source could describe it.
        let unloaded = TestWorld {
            loaded: false,
            ..TestWorld::surface(64)
        };
        assert_eq!(
            find_spawn(
                &unloaded,
                [0.5, 64.0, 0.5],
                SpawnBounds::OVERWORLD,
                &mut Rng::new(19)
            ),
            None
        );
    }

    #[test]
    fn border_edges_are_inclusive_at_the_minimum_and_exclusive_at_the_maximum() {
        let bounds = SpawnBounds {
            border_diameter: 10.0,
            ..SpawnBounds::OVERWORLD
        };
        assert!(bounds.contains_column(-5, -5));
        assert!(bounds.contains_column(4, 4));
        assert!(!bounds.contains_column(5, 0));
        assert!(!bounds.contains_column(0, 5));
        assert!(!bounds.contains_column(-6, 0));

        assert!(!bounds.fits_two_blocks(bounds.min_y - 1));
        assert!(bounds.fits_two_blocks(bounds.min_y));
        assert!(!bounds.fits_two_blocks(bounds.max_y - 1));
        assert!(bounds.fits_two_blocks(bounds.max_y - 2));
    }

    #[test]
    fn trading_pauses_the_despawn_clock_until_the_screen_closes() {
        let mut lifetime = 2;
        assert!(!tick_despawn_delay(&mut lifetime, true));
        assert_eq!(lifetime, 2, "an open trade screen pauses the clock");

        assert!(!tick_despawn_delay(&mut lifetime, false));
        assert_eq!(lifetime, 1);

        assert!(!tick_despawn_delay(&mut lifetime, true));
        assert_eq!(
            lifetime, 1,
            "the last tick is held while the player is still trading"
        );

        assert!(tick_despawn_delay(&mut lifetime, false));
        assert_eq!(lifetime, 0, "closing the screen lets the next tick expire");

        let mut unmanaged = 0;
        assert!(!tick_despawn_delay(&mut unmanaged, false));
        assert_eq!(unmanaged, 0);
    }

    #[test]
    fn the_drink_cycle_is_night_invisibility_and_day_milk() {
        assert_eq!(drink_for(0, false, false), None);
        assert_eq!(drink_for(13_000, false, false), Some(Drink::Invisibility));
        assert_eq!(drink_for(22_999, false, false), Some(Drink::Invisibility));
        assert_eq!(drink_for(23_000, true, false), Some(Drink::Milk));
        assert_eq!(drink_for(23_000, false, false), None);
        assert_eq!(drink_for(13_000, true, false), None);
        // An open trade screen pauses both drink goals until it closes.
        assert_eq!(drink_for(13_000, false, true), None);
        assert_eq!(drink_for(23_000, true, true), None);
    }

    #[test]
    fn drinks_do_not_reset_the_traders_offers_or_trade_uses() {
        let mut trader = new_mob(
            MobKind::WanderingTrader,
            Vec3::new(0.5, 65.0, 0.5),
            &mut Rng::new(3),
        );
        let offers = trader.data.villager.as_ref().unwrap().offers.clone();
        let first_uses = offers[0].uses;

        let applied = apply_drink(&mut trader.data, Drink::Invisibility);
        assert!(applied.added.is_some());
        assert!(trader.data.effects.has(Effect::Invisibility));
        assert_eq!(trader.data.villager.as_ref().unwrap().offers, offers);
        assert_eq!(
            trader.data.villager.as_ref().unwrap().offers[0].uses,
            first_uses
        );

        let applied = apply_drink(&mut trader.data, Drink::Milk);
        assert_eq!(applied.removed.len(), 1);
        assert!(!trader.data.effects.has(Effect::Invisibility));
        assert_eq!(trader.data.villager.as_ref().unwrap().offers, offers);
    }
}
