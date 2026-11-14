//! Daylight burning (vanilla `Mob.isSunBurnTick`) and zombie door breaking (`BreakDoorGoal`).
//!
//! Checked against the 26.3 jar. A sun-sensitive monster (zombie, skeleton) ignites for eight
//! seconds on a tick when the `monsters_burn` environment attribute is on, its eye block has a
//! light-dependent magic value above 0.5 (`level / 15` mapped through `b / (4 - 3b)`, so light
//! 12 or more), `nextFloat() * 30 < (value - 0.4) * 2`, it is not in water or rain, and it can
//! see the sky. Wearing a helmet instead damages the helmet on half the burn ticks; Ferrite's
//! mobs have no equipment, so a helmet is never worn. `BreakDoorGoal` needs `mobGriefing`, the
//! Hard difficulty and a closed wooden door, and breaks the door on its 240th tick.
//!
//! Approximations (see `docs/mob-ai-parity.md`): the door is found by looking one to two blocks
//! toward the target instead of along a path that may open doors, so a zombie that merely stands
//! near a door it does not need is left alone, and one whose route needs a door that the
//! pathfinder cannot cross still stands in front of it; `can_break_doors` is rolled at one in
//! ten at spawn instead of `difficulty special multiplier * 0.1`; the monsters-burn attribute
//! is "the overworld".

use crate::mob::{self, MobKind};
use crate::physics::BlockGetter;
use crate::{EntityKey, EntityStore, Rng};
use ferrite_math::Vec3;

/// Ticks an ignited monster keeps burning (`igniteForSeconds(8)`).
pub const BURN_TICKS: u16 = 160;
/// `BreakDoorGoal.getDoorBreakTime`.
pub const DOOR_BREAK_TICKS: u16 = 240;
/// Chance that a spawned zombie can break doors, standing in for `special multiplier * 0.1`.
pub const CAN_BREAK_DOORS_CHANCE: i32 = 10;
/// How far ahead of itself a zombie looks for the door in its way.
const DOOR_REACH: [f64; 2] = [0.8, 1.6];

/// `LevelReader.getLightLevelDependentMagicValue` from a raw light level 0..=15.
pub fn light_magic_value(level: u8) -> f32 {
    let brightness = f32::from(level) / 15.0;
    brightness / (4.0 - 3.0 * brightness)
}

fn is_air(state: Option<u16>) -> bool {
    state
        .and_then(ferrite_data::blocks::state)
        .is_none_or(|(name, _)| {
            matches!(
                name,
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
        })
}

/// Vanilla `Level.canSeeSky`: nothing but air above the block (the world-surface height map).
pub fn sees_sky(blocks: &impl BlockGetter, x: i32, y: i32, z: i32) -> bool {
    (y + 1..=y + 320).all(|above| is_air(blocks.block_state(x, above, z)))
}

/// Whether a sun-sensitive monster burns this tick (`Mob.isSunBurnTick`). `wet` is
/// `isInWaterOrRain`.
pub fn sun_burn_tick(
    blocks: &impl BlockGetter,
    pos: Vec3,
    eye_height: f64,
    wet: bool,
    rng: &mut Rng,
) -> bool {
    let eye = (pos.y + eye_height).floor() as i32;
    let (x, z) = (pos.x.floor() as i32, pos.z.floor() as i32);
    let Some(level) = blocks.brightness(x, eye, z) else {
        return false;
    };
    let value = light_magic_value(level);
    value > 0.5
        && rng.next_f32() * 30.0 < (value - 0.4) * 2.0
        && !wet
        && sees_sky(blocks, x, eye, z)
}

/// Ignites a sun-sensitive monster that burns this tick. Returns whether it did.
pub fn tick_sun(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    monsters_burn: bool,
    raining: bool,
    rng: &mut Rng,
) -> bool {
    if !monsters_burn {
        return false;
    }
    let Some(state) = mob::state(store, key) else {
        return false;
    };
    if !matches!(state.kind, MobKind::Zombie | MobKind::Skeleton) || state.is_dead() {
        return false;
    }
    let Some(pos) = store.pos(key) else {
        return false;
    };
    let wet = crate::enderman::in_water_or_rain(store, key, blocks, raining);
    let eye_height = f64::from(state.kind.props().eye_height);
    if sun_burn_tick(blocks, pos, eye_height, wet, rng) {
        mob::ignite(store, key, BURN_TICKS);
        return true;
    }
    false
}

/// Vanilla `FleeSunGoal`: an idle skeleton that is on fire under open sky picks a spot out of
/// the sun (up to ten tries, within 10 blocks horizontally and 3 vertically; `getHidePos`) and
/// runs to it. Returns the spot to head for, or `None` when it has nowhere to go.
///
/// `RestrictSunGoal` (path nodes in sunlight avoided) is not modelled: the A* pathfinder has no
/// per-node cost hook.
pub fn tick_flee_sun(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    start: Vec3,
    rng: &mut Rng,
) -> Option<[i32; 3]> {
    let state = mob::state(store, key)?;
    if state.kind != MobKind::Skeleton || state.is_dead() {
        return None;
    }
    let here = [
        start.x.floor() as i32,
        start.y.floor() as i32,
        start.z.floor() as i32,
    ];
    if let Some(shade) = state.shade_pos {
        let close = (shade[0] - here[0]).abs() <= 1
            && (shade[1] - here[1]).abs() <= 1
            && (shade[2] - here[2]).abs() <= 1;
        if !close {
            return Some(shade);
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.shade_pos = None;
        }
    }
    let state = mob::state(store, key)?;
    if state.fire == 0 || !sees_sky(blocks, here[0], here[1] + 1, here[2]) {
        return None;
    }
    for _ in 0..10 {
        let spot = [
            here[0] + rng.next_int(21) - 10,
            here[1] + rng.next_int(7) - 3,
            here[2] + rng.next_int(21) - 10,
        ];
        if !sees_sky(blocks, spot[0], spot[1] + 1, spot[2])
            && crate::path::start_node(blocks, spot) == Some(spot)
        {
            if let Some(state) = mob::state_mut(store, key) {
                state.shade_pos = Some(spot);
            }
            return Some(spot);
        }
    }
    None
}

/// Whether `state` is a closed wooden door (`#minecraft:wooden_doors`, `open=false`).
pub fn is_closed_wooden_door(state: Option<u16>) -> bool {
    state
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, props)| {
            ferrite_data::tags::contains("block", "wooden_doors", name)
                && props.iter().any(|(k, v)| *k == "open" && *v == "false")
        })
}

/// The closed wooden door a zombie heading toward `target` runs into, as its lower half.
pub fn door_ahead(blocks: &impl BlockGetter, pos: Vec3, target: Vec3) -> Option<[i32; 3]> {
    let (dx, dz) = (target.x - pos.x, target.z - pos.z);
    let length = dx.hypot(dz);
    if length < 1e-3 {
        return None;
    }
    let (ux, uz) = (dx / length, dz / length);
    for reach in DOOR_REACH {
        let x = (pos.x + ux * reach).floor() as i32;
        let z = (pos.z + uz * reach).floor() as i32;
        let y = pos.y.floor() as i32;
        for door_y in [y, y - 1] {
            if is_closed_wooden_door(blocks.block_state(x, door_y, z)) {
                let lower = ferrite_data::blocks::state(
                    blocks.block_state(x, door_y, z).unwrap_or_default(),
                )
                .is_some_and(|(_, props)| props.iter().any(|(k, v)| *k == "half" && *v == "lower"));
                return Some([x, if lower { door_y } else { door_y - 1 }, z]);
            }
        }
    }
    None
}

/// What the break-door goal wants after one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorStep {
    /// Nothing to break, or the progress was reset.
    Idle,
    /// Chewing on the door with no visible change this tick.
    Chewing,
    /// The crack stage (vanilla 0..=9) advanced; the stage changes every 24 ticks.
    Breaking { at: [i32; 3], progress: u8 },
    /// The 240th tick: the door is gone.
    Broken { at: [i32; 3] },
}

/// One tick of `BreakDoorGoal` for a zombie chasing `target`. `hard` and `griefing` are the
/// Hard difficulty and `mobGriefing`.
pub fn tick_break_door(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    target: Vec3,
    hard: bool,
    griefing: bool,
) -> DoorStep {
    let Some(state) = mob::state(store, key).copied() else {
        return DoorStep::Idle;
    };
    let Some(pos) = store.pos(key) else {
        return DoorStep::Idle;
    };
    let door = (state.kind == MobKind::Zombie && state.can_break_doors && hard && griefing)
        .then(|| door_ahead(blocks, pos, target))
        .flatten();
    let Some(state) = mob::state_mut(store, key) else {
        return DoorStep::Idle;
    };
    let Some(at) = door else {
        state.door_pos = None;
        state.door_ticks = 0;
        return DoorStep::Idle;
    };
    if state.door_pos != Some(at) {
        state.door_pos = Some(at);
        state.door_ticks = 0;
    }
    state.door_ticks += 1;
    if state.door_ticks >= DOOR_BREAK_TICKS {
        state.door_pos = None;
        state.door_ticks = 0;
        return DoorStep::Broken { at };
    }
    let stage = |ticks: u16| (f32::from(ticks) / f32::from(DOOR_BREAK_TICKS) * 10.0) as u8;
    let progress = stage(state.door_ticks);
    if state.door_ticks == 1 || progress != stage(state.door_ticks - 1) {
        DoorStep::Breaking { at, progress }
    } else {
        DoorStep::Chewing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{self, MobEvent, MobRules, Target};
    use uuid::Uuid;

    /// Stone below y = 64, light 15 above it (or `light`), a door in a chosen column, and an
    /// optional roof.
    struct World {
        light: u8,
        roof: Option<i32>,
        door: Option<(i32, bool)>,
        /// A roof at y = 70 over every column with `x >= shade_x`.
        shade_x: Option<i32>,
    }

    impl World {
        fn day() -> Self {
            Self {
                light: 15,
                roof: None,
                door: None,
                shade_x: None,
            }
        }
    }

    fn door_state(half: &str, open: bool) -> Option<u16> {
        ferrite_data::blocks::state_id(
            "minecraft:oak_door",
            &[
                ("facing", "north"),
                ("half", half),
                ("hinge", "left"),
                ("open", if open { "true" } else { "false" }),
                ("powered", "false"),
            ],
        )
    }

    impl BlockGetter for World {
        fn block_state(&self, x: i32, y: i32, _z: i32) -> Option<u16> {
            if y < 64 || self.roof == Some(y) || (y == 70 && self.shade_x.is_some_and(|s| x >= s)) {
                return ferrite_data::blocks::default_state("stone");
            }
            if let Some((door_x, open)) = self.door
                && x == door_x
                && (y == 64 || y == 65)
            {
                return door_state(if y == 64 { "lower" } else { "upper" }, open);
            }
            ferrite_data::blocks::default_state("air")
        }

        fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
            Some(self.light)
        }
    }

    fn spawn(kind: MobKind, x: f64) -> (EntityStore, EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(mob::new_mob(
            kind,
            Vec3::new(x, 64.0, 0.5),
            &mut Rng::new(2),
        ));
        (store, key)
    }

    #[test]
    fn light_12_is_where_the_magic_value_passes_one_half() {
        assert!(light_magic_value(12) > 0.5 - 1e-6 && light_magic_value(11) < 0.5);
        assert!((light_magic_value(15) - 1.0).abs() < 1e-6);
        assert_eq!(light_magic_value(0), 0.0);
    }

    #[test]
    fn full_daylight_ignites_about_four_percent_of_ticks() {
        let world = World::day();
        let mut rng = Rng::new(10);
        let pos = Vec3::new(0.5, 64.0, 0.5);
        let burns = (0..50_000)
            .filter(|_| sun_burn_tick(&world, pos, 1.74, false, &mut rng))
            .count();
        // nextFloat * 30 < (1 - 0.4) * 2 = 1.2, so 4%.
        let share = burns as f64 / 50_000.0;
        assert!((share - 0.04).abs() < 0.006, "burn share {share}");
    }

    #[test]
    fn dim_light_water_and_a_roof_stop_the_burn() {
        let mut rng = Rng::new(10);
        let pos = Vec3::new(0.5, 64.0, 0.5);
        let dim = World {
            light: 11,
            ..World::day()
        };
        let roofed = World {
            roof: Some(70),
            ..World::day()
        };
        for _ in 0..5_000 {
            assert!(!sun_burn_tick(&dim, pos, 1.74, false, &mut rng), "dim");
            assert!(!sun_burn_tick(&roofed, pos, 1.74, false, &mut rng), "roof");
            assert!(
                !sun_burn_tick(&World::day(), pos, 1.74, true, &mut rng),
                "wet"
            );
        }
    }

    #[test]
    fn zombies_and_skeletons_ignite_for_eight_seconds_but_creepers_and_spiders_do_not() {
        for (kind, burns) in [
            (MobKind::Zombie, true),
            (MobKind::Skeleton, true),
            (MobKind::Creeper, false),
            (MobKind::Spider, false),
            (MobKind::Enderman, false),
        ] {
            let (mut store, key) = spawn(kind, 0.5);
            let mut rng = Rng::new(3);
            let lit =
                (0..2_000).any(|_| tick_sun(&mut store, key, &World::day(), true, false, &mut rng));
            assert_eq!(lit, burns, "{kind:?}");
            if burns {
                assert_eq!(mob::state(&store, key).unwrap().fire, BURN_TICKS);
            }
        }
    }

    #[test]
    fn nothing_burns_where_monsters_do_not() {
        let (mut store, key) = spawn(MobKind::Zombie, 0.5);
        let mut rng = Rng::new(3);
        for _ in 0..2_000 {
            assert!(!tick_sun(
                &mut store,
                key,
                &World::day(),
                false,
                false,
                &mut rng
            ));
        }
        assert_eq!(mob::state(&store, key).unwrap().fire, 0);
    }

    /// A player 60 blocks away: far enough not to be a target (follow range 35) but near
    /// enough that hostile mobs do not despawn for lack of one.
    fn distant_player() -> [Target; 1] {
        [Target {
            uuid: Uuid::from_u128(77),
            pos: Vec3::new(60.5, 64.0, 0.5),
        }]
    }

    #[test]
    fn a_zombie_in_daylight_burns_and_loses_health_through_the_ai() {
        let (mut store, key) = spawn(MobKind::Zombie, 0.5);
        let rules = MobRules {
            monsters_burn: true,
            ..Default::default()
        };
        let mut rng = Rng::new(4);
        let before = mob::state(&store, key).unwrap().health;
        for _ in 0..400 {
            ai::tick_full(
                &mut store,
                &World::day(),
                &distant_player(),
                &rules,
                &[],
                &[],
                &mut rng,
            );
        }
        let after = mob::state(&store, key).map_or(0.0, |s| s.health);
        assert!(
            after < before,
            "a zombie in full daylight must burn: {after}"
        );
        // The same zombie at night (dim light) stays whole.
        let (mut store, key) = spawn(MobKind::Zombie, 0.5);
        let night = World {
            light: 4,
            ..World::day()
        };
        for _ in 0..400 {
            ai::tick_full(
                &mut store,
                &night,
                &distant_player(),
                &rules,
                &[],
                &[],
                &mut rng,
            );
        }
        assert_eq!(mob::state(&store, key).unwrap().health, before);
    }

    fn door_zombie(open: bool) -> (EntityStore, EntityKey, World) {
        let (mut store, key) = spawn(MobKind::Zombie, 0.5);
        mob::state_mut(&mut store, key).unwrap().can_break_doors = true;
        let world = World {
            door: Some((1, open)),
            ..World::day()
        };
        (store, key, world)
    }

    const TARGET: Vec3 = Vec3::new(5.5, 64.0, 0.5);

    #[test]
    fn a_closed_wooden_door_ahead_is_found_and_an_open_or_distant_one_is_not() {
        let pos = Vec3::new(0.5, 64.0, 0.5);
        let (_, _, closed) = door_zombie(false);
        assert_eq!(door_ahead(&closed, pos, TARGET), Some([1, 64, 0]));
        let (_, _, open) = door_zombie(true);
        assert_eq!(door_ahead(&open, pos, TARGET), None);
        let far = Vec3::new(-5.0, 64.0, 0.5);
        assert_eq!(door_ahead(&closed, far, TARGET), None);
        // Facing away from the door.
        assert_eq!(door_ahead(&closed, pos, Vec3::new(-5.5, 64.0, 0.5)), None);
    }

    #[test]
    fn the_door_breaks_on_the_240th_tick_on_hard_with_griefing() {
        let (mut store, key, world) = door_zombie(false);
        for tick in 1..DOOR_BREAK_TICKS {
            let step = tick_break_door(&mut store, key, &world, TARGET, true, true);
            assert_ne!(step, DoorStep::Idle, "tick {tick}");
            assert!(!matches!(step, DoorStep::Broken { .. }), "tick {tick}");
        }
        assert_eq!(
            tick_break_door(&mut store, key, &world, TARGET, true, true),
            DoorStep::Broken { at: [1, 64, 0] }
        );
        assert_eq!(mob::state(&store, key).unwrap().door_ticks, 0);
    }

    #[test]
    fn the_crack_stage_advances_every_24_ticks() {
        let (mut store, key, world) = door_zombie(false);
        let mut stages = Vec::new();
        for _ in 1..DOOR_BREAK_TICKS {
            if let DoorStep::Breaking { progress, .. } =
                tick_break_door(&mut store, key, &world, TARGET, true, true)
            {
                stages.push(progress);
            }
        }
        assert_eq!(stages, (0..10).collect::<Vec<u8>>());
    }

    #[test]
    fn no_hard_difficulty_no_griefing_or_no_door_skill_means_no_progress() {
        for (hard, griefing, skill) in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            let (mut store, key, world) = door_zombie(false);
            mob::state_mut(&mut store, key).unwrap().can_break_doors = skill;
            for _ in 0..300 {
                assert_eq!(
                    tick_break_door(&mut store, key, &world, TARGET, hard, griefing),
                    DoorStep::Idle
                );
            }
        }
    }

    #[test]
    fn the_progress_resets_when_the_door_opens() {
        let (mut store, key, closed) = door_zombie(false);
        for _ in 0..100 {
            tick_break_door(&mut store, key, &closed, TARGET, true, true);
        }
        assert_eq!(mob::state(&store, key).unwrap().door_ticks, 100);
        let (_, _, open) = door_zombie(true);
        assert_eq!(
            tick_break_door(&mut store, key, &open, TARGET, true, true),
            DoorStep::Idle
        );
        assert_eq!(mob::state(&store, key).unwrap().door_ticks, 0);
    }

    #[test]
    fn only_zombies_break_doors_and_only_one_in_ten_spawn_able_to() {
        let (mut store, key) = spawn(MobKind::Skeleton, 0.5);
        mob::state_mut(&mut store, key).unwrap().can_break_doors = true;
        let world = World {
            door: Some((1, false)),
            ..World::day()
        };
        assert_eq!(
            tick_break_door(&mut store, key, &world, TARGET, true, true),
            DoorStep::Idle
        );
        let mut rng = Rng::new(99);
        let able = (0..4_000)
            .filter(|_| {
                let record = mob::new_mob(MobKind::Zombie, Vec3::new(0.5, 64.0, 0.5), &mut rng);
                record.data.mob.unwrap().can_break_doors
            })
            .count();
        assert!((300..500).contains(&able), "{able} of 4000");
    }

    #[test]
    fn a_chasing_zombie_emits_the_break_event_through_the_ai() {
        let (mut store, key, world) = door_zombie(false);
        let rules = MobRules {
            hard: true,
            griefing: true,
            ..Default::default()
        };
        let player = Target {
            uuid: Uuid::from_u128(1),
            pos: TARGET,
        };
        let mut rng = Rng::new(7);
        let mut broke = None;
        for tick in 1..=400 {
            // The zombie keeps getting pushed back to the door: put it back every tick.
            store.set_pos(key, Vec3::new(0.5, 64.0, 0.5));
            for event in ai::tick_full(&mut store, &world, &[player], &rules, &[], &[], &mut rng) {
                if let MobEvent::BreakDoor {
                    at, broken: true, ..
                } = event
                {
                    broke = Some((tick, at));
                }
            }
            if broke.is_some() {
                break;
            }
        }
        assert_eq!(broke, Some((DOOR_BREAK_TICKS as i32, [1, 64, 0])));
    }

    fn shaded() -> World {
        World {
            shade_x: Some(4),
            ..World::day()
        }
    }

    #[test]
    fn a_burning_idle_skeleton_picks_a_shaded_spot_and_walks_toward_it() {
        let world = shaded();
        let (mut store, key) = spawn(MobKind::Skeleton, 0.5);
        mob::ignite(&mut store, key, 100);
        let mut rng = Rng::new(5);
        let rules = MobRules::default();
        let mut shade = None;
        for _ in 0..40 {
            ai::tick_full(
                &mut store,
                &world,
                &distant_player(),
                &rules,
                &[],
                &[],
                &mut rng,
            );
            shade = shade.or(mob::state(&store, key).and_then(|s| s.shade_pos));
        }
        let shade = shade.expect("a shaded spot was chosen");
        assert!(shade[0] >= 4, "{shade:?} is under the roof");
        assert!(
            store.pos(key).unwrap().x > 0.5,
            "the skeleton moved toward the shade"
        );
    }

    #[test]
    fn a_skeleton_that_is_not_burning_stays_put_and_picks_nothing() {
        let world = shaded();
        let (mut store, key) = spawn(MobKind::Skeleton, 0.5);
        let mut rng = Rng::new(5);
        let start = store.pos(key).unwrap();
        assert_eq!(
            tick_flee_sun(&mut store, key, &world, start, &mut rng),
            None
        );
        assert!(mob::state(&store, key).unwrap().shade_pos.is_none());
    }

    #[test]
    fn a_burning_skeleton_under_cover_does_not_flee() {
        let world = World {
            roof: Some(70),
            ..World::day()
        };
        let (mut store, key) = spawn(MobKind::Skeleton, 0.5);
        mob::ignite(&mut store, key, 100);
        let start = store.pos(key).unwrap();
        let mut rng = Rng::new(5);
        assert_eq!(
            tick_flee_sun(&mut store, key, &world, start, &mut rng),
            None
        );
    }

    #[test]
    fn a_zombie_never_flees_the_sun() {
        let world = shaded();
        let (mut store, key) = spawn(MobKind::Zombie, 0.5);
        mob::ignite(&mut store, key, 100);
        let start = store.pos(key).unwrap();
        let mut rng = Rng::new(5);
        assert_eq!(
            tick_flee_sun(&mut store, key, &world, start, &mut rng),
            None
        );
    }
}
