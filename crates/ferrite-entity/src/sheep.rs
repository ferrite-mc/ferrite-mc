//! Sheep (vanilla `Sheep`): wool colour, the sheared flag, `EatBlockGoal` and shearing.
//!
//! Checked against the 26.3 jar: `DATA_WOOL_ID` is synched byte 18 (colour in the low four bits,
//! sheared as `0x10`), the legacy colour ids are the `DyeColor` order, `readyForShearing` is
//! "not sheared and not a baby", `EatBlockGoal` starts one tick in 1000 (one in 50 for a baby),
//! runs 40 ticks and eats on the tick the counter reaches 4, and `ate()` regrows the wool and
//! ages a baby by 60 ticks.

use crate::ai::MobEvent;
use crate::mob::{self, MobKind, MobState};
use crate::physics::BlockGetter;
use crate::{EntityKey, EntityStore, Metadata, Rng};
use ferrite_protocol::packets::play::MetaValue;

/// Synched `Sheep.DATA_WOOL_ID` (`AgeableMob` owns 16 and 17).
pub const WOOL_INDEX: u8 = 18;
const SHEARED_BIT: i8 = 0x10;
/// `DyeColor` ids, which are also the saved legacy `Color` byte.
pub const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];
/// `EatBlockGoal`: ticks the goal runs, and the tick the block is eaten on.
pub const EAT_TICKS: u8 = 40;
pub const EAT_AT: u8 = 4;
/// `Sheep.ate` ages a baby by this many ticks.
pub const ATE_AGE_UP: i32 = 60;
/// Entity event that starts the head-down eating animation on the client.
pub const EAT_EVENT: u8 = 10;
/// The legacy colour ids of the wool item names.
pub fn color_name(id: u8) -> &'static str {
    COLORS[usize::from(id & 15)]
}

/// The colour id for a dye name such as `light_blue`.
pub fn color_id(name: &str) -> Option<u8> {
    let name = name.trim_start_matches("minecraft:");
    COLORS.iter().position(|c| *c == name).map(|i| i as u8)
}

/// The wool colour of a dye item (`minecraft:red_dye` is 14); `None` for any other item. Every
/// vanilla dye is `<colour>_dye` in `DyeColor` order, which is also the wool colour id.
pub fn dye_color(item: &str) -> Option<u8> {
    color_id(item.strip_prefix("minecraft:")?.strip_suffix("_dye")?)
}

/// `DyeItem.interactLivingEntity` on a sheep: a live, unsheared sheep whose colour differs
/// takes the dye's colour. Returns whether it changed; the caller plays the sound and takes the
/// dye. A sheared sheep or one already of that
/// colour leaves the click unused (babies can be dyed: vanilla only checks `isSheared`).
pub fn dye(store: &mut EntityStore, key: EntityKey, color: u8) -> bool {
    let Some(state) = mob::state(store, key) else {
        return false;
    };
    if state.kind != MobKind::Sheep
        || state.sheared
        || state.health <= 0.0
        || state.wool_color == color & 15
    {
        return false;
    }
    set_color(store, key, color);
    true
}

/// Vanilla `SheepColorSpawnRules` in a temperate biome: 5% black, gray and light gray each, 3%
/// brown and 82% white (of which one in 500 is pink). Warm and cold biome tables need the biome
/// at the spawn point, which the entity crate does not have, so every sheep uses this table.
pub fn random_color(rng: &mut Rng) -> u8 {
    match rng.next_int(100) {
        0..=4 => 15,
        5..=9 => 7,
        10..=14 => 8,
        15..=17 => 12,
        _ if rng.next_int(500) == 0 => 6,
        _ => 0,
    }
}

/// The two-dye crafting recipes whose result is a dye (`DyeColor.findColorMixInRecipes`).
fn mix(a: u8, b: u8) -> Option<u8> {
    let pair = |x: &str, y: &str| {
        let (x, y) = (color_id(x)?, color_id(y)?);
        ((a == x && b == y) || (a == y && b == x)).then_some(())
    };
    let table = [
        ("blue", "green", "cyan"),
        ("black", "white", "gray"),
        ("blue", "white", "light_blue"),
        ("gray", "white", "light_gray"),
        ("green", "white", "lime"),
        ("purple", "pink", "magenta"),
        ("red", "yellow", "orange"),
        ("red", "white", "pink"),
        ("blue", "red", "purple"),
    ];
    table
        .iter()
        .find(|(x, y, _)| pair(x, y).is_some())
        .and_then(|(_, _, result)| color_id(result))
}

/// A baby's colour (`DyeColor.getMixedColor`): the recipe mix of the parents, else either
/// parent's colour with equal chance.
pub fn offspring_color(a: u8, b: u8, rng: &mut Rng) -> u8 {
    mix(a, b).unwrap_or(if rng.next_int(2) == 0 { a } else { b })
}

/// Defines the wool byte on a sheep's synched data and writes the current state into it.
pub fn define_metadata(metadata: &mut Metadata, state: &MobState) {
    metadata.define(WOOL_INDEX, MetaValue::Byte(0));
    write_metadata(metadata, state);
}

fn write_metadata(metadata: &mut Metadata, state: &MobState) {
    let mut byte = (state.wool_color & 15) as i8;
    if state.sheared {
        byte |= SHEARED_BIT;
    }
    metadata.set(WOOL_INDEX, MetaValue::Byte(byte));
}

/// Re-writes the wool byte after the colour or sheared state changed.
pub fn sync(store: &mut EntityStore, key: EntityKey) {
    if let Some(data) = store.data_mut(key)
        && let Some(state) = data.mob
        && state.kind == MobKind::Sheep
    {
        write_metadata(&mut data.metadata, &state);
    }
}

/// Sets a sheep's colour (a dye use or a spawn).
pub fn set_color(store: &mut EntityStore, key: EntityKey, color: u8) {
    if let Some(state) = mob::state_mut(store, key)
        && state.kind == MobKind::Sheep
    {
        state.wool_color = color & 15;
        sync(store, key);
    }
}

/// Vanilla `Sheep.readyForShearing`: wool that has grown and an adult.
pub fn ready_for_shearing(store: &EntityStore, key: EntityKey) -> bool {
    mob::state(store, key).is_some_and(|s| s.kind == MobKind::Sheep && !s.sheared && s.age >= 0)
}

/// Shears a sheep that is ready: it is sheared from now on. Returns the colour it had, which
/// picks the `shearing/sheep/<colour>` loot table, or `None` when nothing happened.
pub fn shear(store: &mut EntityStore, key: EntityKey) -> Option<u8> {
    if !ready_for_shearing(store, key) {
        return None;
    }
    let state = mob::state_mut(store, key)?;
    state.sheared = true;
    let color = state.wool_color;
    sync(store, key);
    Some(color)
}

/// The loot predicate facts of a sheep (`components/minecraft:sheep/color` and
/// `type_specific/sheep/sheared`), for both the death and the shearing tables.
pub fn loot_properties(state: &MobState) -> Vec<(String, String)> {
    vec![
        (
            "components/minecraft:sheep/color".to_owned(),
            color_name(state.wool_color).to_owned(),
        ),
        (
            "type_specific/sheep/sheared".to_owned(),
            state.sheared.to_string(),
        ),
    ]
}

/// Whether a block is eaten from where the sheep stands (`#minecraft:edible_for_sheep`).
fn edible(state: Option<u16>) -> bool {
    state
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, _)| ferrite_data::tags::contains("block", "edible_for_sheep", name))
}

fn is_grass_block(state: Option<u16>) -> bool {
    state
        .and_then(ferrite_data::blocks::state)
        .is_some_and(|(name, _)| name == "minecraft:grass_block")
}

/// `Sheep.ate`: the wool grows back and a baby grows up a little.
pub fn ate(store: &mut EntityStore, key: EntityKey) {
    if let Some(state) = mob::state_mut(store, key) {
        state.sheared = false;
        if state.age < 0 {
            state.age = (state.age + ATE_AGE_UP).min(0);
        }
    }
    sync(store, key);
}

/// Advances the eat goal for one sheep and reports whether it is eating (and so standing
/// still). Vanilla lets `PanicGoal` (priority 1) pre-empt it; Ferrite checks only panic, so a
/// sheep that is in love or following a parent can still start to eat.
pub fn tick_eat(
    store: &mut EntityStore,
    key: EntityKey,
    blocks: &impl BlockGetter,
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) -> bool {
    let Some(state) = mob::state(store, key).copied() else {
        return false;
    };
    let Some(pos) = store.pos(key) else {
        return false;
    };
    let at = [
        pos.x.floor() as i32,
        pos.y.floor() as i32,
        pos.z.floor() as i32,
    ];
    if state.eat_timer == 0 {
        if state.panic > 0 {
            return false;
        }
        let bound = if state.age < 0 { 50 } else { 1000 };
        if rng.next_int(bound) != 0 {
            return false;
        }
        let here = blocks.block_state(at[0], at[1], at[2]);
        let below = blocks.block_state(at[0], at[1] - 1, at[2]);
        if !edible(here) && !is_grass_block(below) {
            return false;
        }
        if let Some(state) = mob::state_mut(store, key) {
            state.eat_timer = EAT_TICKS;
            state.path_len = 0;
            state.path_index = 0;
        }
        events.push(MobEvent::Status {
            key,
            status: EAT_EVENT,
        });
        return true;
    }
    let timer = state.eat_timer.saturating_sub(1);
    if let Some(state) = mob::state_mut(store, key) {
        state.eat_timer = timer;
    }
    if timer == EAT_AT {
        let here = blocks.block_state(at[0], at[1], at[2]);
        let below = blocks.block_state(at[0], at[1] - 1, at[2]);
        if edible(here) {
            events.push(MobEvent::EatBlock { at, grass: false });
            ate(store, key);
        } else if is_grass_block(below) {
            events.push(MobEvent::EatBlock {
                at: [at[0], at[1] - 1, at[2]],
                grass: true,
            });
            ate(store, key);
        }
    }
    timer > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_math::Vec3;

    /// Grass block floor at y = 63 with `plant` (or air) at y = 64.
    struct Meadow {
        plant: &'static str,
        floor: &'static str,
    }

    impl BlockGetter for Meadow {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            let name = match y {
                ..=62 => "stone",
                63 => self.floor,
                64 => self.plant,
                _ => "air",
            };
            ferrite_data::blocks::default_state(name)
        }
    }

    fn sheep(store: &mut EntityStore, rng: &mut Rng) -> EntityKey {
        store.insert(mob::new_mob(MobKind::Sheep, Vec3::new(0.5, 64.0, 0.5), rng))
    }

    fn wool_byte(store: &EntityStore, key: EntityKey) -> i8 {
        match store.data(key).unwrap().metadata.get(WOOL_INDEX) {
            Some(MetaValue::Byte(byte)) => *byte,
            other => panic!("wool metadata was {other:?}"),
        }
    }

    #[test]
    fn colour_ids_follow_the_dye_order() {
        assert_eq!(color_id("white"), Some(0));
        assert_eq!(color_id("minecraft:light_blue"), Some(3));
        assert_eq!(color_id("black"), Some(15));
        assert_eq!(color_name(14), "red");
        assert_eq!(color_id("teal"), None);
    }

    #[test]
    fn every_dye_item_maps_to_its_wool_colour_and_other_items_to_none() {
        for (id, name) in COLORS.iter().enumerate() {
            assert_eq!(dye_color(&format!("minecraft:{name}_dye")), Some(id as u8));
        }
        assert_eq!(dye_color("minecraft:red_wool"), None);
        assert_eq!(dye_color("minecraft:glow_ink_sac"), None);
        assert_eq!(dye_color("minecraft:bone_meal"), None);
    }

    #[test]
    fn dyeing_changes_the_colour_and_the_wool_metadata_but_not_a_sheared_or_same_colour_sheep() {
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        set_color(&mut store, key, 0);
        assert!(!dye(&mut store, key, 0), "already white");
        assert!(dye(&mut store, key, 14));
        assert_eq!(mob::state(&store, key).unwrap().wool_color, 14);
        assert_eq!(wool_byte(&store, key), 14);
        mob::state_mut(&mut store, key).unwrap().sheared = true;
        assert!(!dye(&mut store, key, 3), "a sheared sheep keeps its colour");
        assert_eq!(mob::state(&store, key).unwrap().wool_color, 14);
        let cow = store.insert(mob::new_mob(
            MobKind::Cow,
            Vec3::new(3.5, 64.0, 0.5),
            &mut rng,
        ));
        assert!(!dye(&mut store, cow, 3));
    }

    #[test]
    fn a_new_sheep_has_a_colour_and_synched_wool_data() {
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        let color = mob::state(&store, key).unwrap().wool_color;
        assert_eq!(wool_byte(&store, key), color as i8);
        set_color(&mut store, key, 14);
        assert_eq!(wool_byte(&store, key), 14);
    }

    #[test]
    fn spawn_colours_follow_the_temperate_table() {
        let mut rng = Rng::new(77);
        let mut counts = [0u32; 16];
        for _ in 0..20_000 {
            counts[usize::from(random_color(&mut rng))] += 1;
        }
        let share = |id: usize| f64::from(counts[id]) / 20_000.0;
        assert!((share(0) - 0.82).abs() < 0.02, "white {}", share(0));
        assert!((share(15) - 0.05).abs() < 0.01, "black {}", share(15));
        assert!((share(7) - 0.05).abs() < 0.01, "gray {}", share(7));
        assert!((share(8) - 0.05).abs() < 0.01, "light gray {}", share(8));
        assert!((share(12) - 0.03).abs() < 0.01, "brown {}", share(12));
        assert_eq!(counts[1] + counts[2] + counts[3] + counts[4] + counts[5], 0);
    }

    #[test]
    fn shearing_marks_the_sheep_and_a_second_shear_does_nothing() {
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        set_color(&mut store, key, 11);
        assert!(ready_for_shearing(&store, key));
        assert_eq!(shear(&mut store, key), Some(11));
        assert!(mob::state(&store, key).unwrap().sheared);
        // Colour bits stay, the sheared bit is set in the synched byte.
        assert_eq!(wool_byte(&store, key), 11 | 0x10);
        assert!(!ready_for_shearing(&store, key));
        assert_eq!(shear(&mut store, key), None);
    }

    #[test]
    fn a_lamb_cannot_be_sheared() {
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        mob::state_mut(&mut store, key).unwrap().age = -100;
        assert!(!ready_for_shearing(&store, key));
        assert_eq!(shear(&mut store, key), None);
        assert!(!mob::state(&store, key).unwrap().sheared);
    }

    #[test]
    fn loot_facts_name_the_colour_and_the_sheared_state() {
        let mut rng = Rng::new(5);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        set_color(&mut store, key, 4);
        let facts = loot_properties(mob::state(&store, key).unwrap());
        assert!(facts.contains(&("components/minecraft:sheep/color".into(), "yellow".into())));
        assert!(facts.contains(&("type_specific/sheep/sheared".into(), "false".into())));
    }

    #[test]
    fn offspring_mix_the_recipe_colours_and_otherwise_take_a_parent() {
        let mut rng = Rng::new(3);
        let red = color_id("red").unwrap();
        let yellow = color_id("yellow").unwrap();
        let orange = color_id("orange").unwrap();
        assert_eq!(offspring_color(red, yellow, &mut rng), orange);
        assert_eq!(offspring_color(yellow, red, &mut rng), orange);
        let blue = color_id("blue").unwrap();
        let white = color_id("white").unwrap();
        assert_eq!(
            offspring_color(blue, white, &mut rng),
            color_id("light_blue").unwrap()
        );
        // Black and red have no two-dye recipe: either parent's colour, both seen.
        let (black, red) = (color_id("black").unwrap(), color_id("red").unwrap());
        let seen: std::collections::BTreeSet<u8> = (0..64)
            .map(|_| offspring_color(black, red, &mut rng))
            .collect();
        assert_eq!(seen, [red, black].into_iter().collect());
        assert_eq!(offspring_color(red, red, &mut rng), red);
    }

    /// Runs the eat goal until it starts (the roll is one in 1000) and returns the events.
    fn start_eating(
        store: &mut EntityStore,
        key: EntityKey,
        blocks: &Meadow,
        rng: &mut Rng,
    ) -> Vec<MobEvent> {
        for _ in 0..200_000 {
            let mut events = Vec::new();
            if tick_eat(store, key, blocks, rng, &mut events) {
                return events;
            }
        }
        panic!("the sheep never started eating");
    }

    #[test]
    fn a_sheep_on_grass_starts_the_goal_with_the_eat_event() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        let meadow = Meadow {
            plant: "air",
            floor: "grass_block",
        };
        let events = start_eating(&mut store, key, &meadow, &mut rng);
        assert_eq!(
            events,
            vec![MobEvent::Status {
                key,
                status: EAT_EVENT
            }]
        );
        assert_eq!(mob::state(&store, key).unwrap().eat_timer, EAT_TICKS);
    }

    #[test]
    fn a_sheep_on_bare_stone_never_starts_eating() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        let rock = Meadow {
            plant: "air",
            floor: "stone",
        };
        for _ in 0..50_000 {
            assert!(!tick_eat(&mut store, key, &rock, &mut rng, &mut Vec::new()));
        }
    }

    #[test]
    fn eating_grass_turns_the_block_to_dirt_on_the_36th_tick_and_regrows_the_wool() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        let meadow = Meadow {
            plant: "air",
            floor: "grass_block",
        };
        shear(&mut store, key);
        start_eating(&mut store, key, &meadow, &mut rng);
        let mut eaten_on = None;
        for tick in 1..=EAT_TICKS {
            let mut events = Vec::new();
            let eating = tick_eat(&mut store, key, &meadow, &mut rng, &mut events);
            if events
                .iter()
                .any(|e| matches!(e, MobEvent::EatBlock { .. }))
            {
                assert_eq!(
                    events,
                    vec![MobEvent::EatBlock {
                        at: [0, 63, 0],
                        grass: true
                    }]
                );
                eaten_on = Some(tick);
            }
            assert_eq!(eating, tick < EAT_TICKS, "tick {tick}");
        }
        // 40 -> 4 takes 36 decrements.
        assert_eq!(eaten_on, Some(EAT_TICKS - EAT_AT));
        assert!(!mob::state(&store, key).unwrap().sheared, "wool regrew");
        assert_eq!(wool_byte(&store, key) & 0x10, 0);
        assert_eq!(mob::state(&store, key).unwrap().eat_timer, 0);
    }

    #[test]
    fn eating_a_short_grass_plant_removes_it_and_leaves_the_grass_alone() {
        let mut rng = Rng::new(11);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        let meadow = Meadow {
            plant: "short_grass",
            floor: "stone",
        };
        start_eating(&mut store, key, &meadow, &mut rng);
        let mut ate_event = None;
        for _ in 0..EAT_TICKS {
            let mut events = Vec::new();
            tick_eat(&mut store, key, &meadow, &mut rng, &mut events);
            if let Some(event) = events
                .into_iter()
                .find(|e| matches!(e, MobEvent::EatBlock { .. }))
            {
                ate_event = Some(event);
            }
        }
        assert_eq!(
            ate_event,
            Some(MobEvent::EatBlock {
                at: [0, 64, 0],
                grass: false
            })
        );
    }

    #[test]
    fn a_lamb_grows_up_sixty_ticks_when_it_eats() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        mob::state_mut(&mut store, key).unwrap().age = -24_000;
        ate(&mut store, key);
        assert_eq!(mob::state(&store, key).unwrap().age, -24_000 + ATE_AGE_UP);
        mob::state_mut(&mut store, key).unwrap().age = -30;
        ate(&mut store, key);
        assert_eq!(mob::state(&store, key).unwrap().age, 0);
    }

    #[test]
    fn a_panicking_sheep_does_not_start_eating() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        mob::state_mut(&mut store, key).unwrap().panic = 40;
        let meadow = Meadow {
            plant: "air",
            floor: "grass_block",
        };
        for _ in 0..50_000 {
            assert!(!tick_eat(
                &mut store,
                key,
                &meadow,
                &mut rng,
                &mut Vec::new()
            ));
        }
    }

    #[test]
    fn the_ai_holds_an_eating_sheep_still() {
        let mut rng = Rng::new(9);
        let mut store = EntityStore::default();
        let key = sheep(&mut store, &mut rng);
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.eat_timer = EAT_TICKS;
            state.moving = true;
            state.wander_timer = 1_000;
        }
        let meadow = Meadow {
            plant: "air",
            floor: "grass_block",
        };
        let start = store.pos(key).unwrap();
        for _ in 0..20 {
            crate::ai::tick_with(&mut store, &meadow, &[], &mut rng);
        }
        let end = store.pos(key).unwrap();
        assert!(
            (end.x - start.x).hypot(end.z - start.z) < 1e-6,
            "an eating sheep moved from {start:?} to {end:?}"
        );
    }
}
