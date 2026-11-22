//! Trial spawners (`TrialSpawnerBlockEntity`).
//!
//! The block state is authoritative for the client: `ominous` selects the reward table and
//! `trial_spawner_state` drives the shutter, flame and loot animations. A spawner is detected
//! when a player walks near it underground, or when a player carrying Trial Omen is nearby; the
//! omen turns the spawner ominous and is consumed. The active spawner then runs three waves of
//! trial mobs, ejects the matching trial-chamber loot and enters vanilla's long cooldown.
//!
//! Only spawners near a player are ticked. Discovery is deliberately gated: normal players are
//! scanned only below y=40 (trial chambers are underground), so the 200-bot surface benchmark
//! never walks the 33x17x33 block volume. Trial Omen is rare enough to scan anywhere.

use std::collections::{HashMap, HashSet};

use ferrite_entity::effect::Effect;
use ferrite_entity::mob::{self, MobKind};
use ferrite_entity::{EntityStore, Flags, Rng};
use ferrite_math::Vec3;
use uuid::Uuid;

use super::anvil::AnvilChunkSource;
use super::region::Player;

/// Vanilla `TrialSpawner` detection box: players within 14 blocks wake the spawner.
pub const DETECTION_RANGE: f64 = 14.0;
/// A normal trial runs three waves.
pub const WAVES: u32 = 3;
/// Mobs in each wave.
pub const MOBS_PER_WAVE: usize = 2;
/// Ticks between a cleared wave and the next one (`TrialSpawner` between-wave delay).
pub const WAVE_DELAY_TICKS: u32 = 20;
/// Delay between the last mob dying and the reward shutter opening.
pub const EJECT_DELAY_TICKS: u32 = 20;
/// Vanilla cooldown after a completed trial (`TrialSpawner.COOLDOWN_TICKS`, 30 minutes).
pub const COOLDOWN_TICKS: u32 = 36_000;
/// How often nearby blocks are scanned.
pub const SCAN_INTERVAL: u32 = 20;
/// Discovery radius for a normal player; trial chambers are underground.
pub const SCAN_RADIUS: i32 = 16;
pub const SCAN_Y: i32 = 8;
/// Players above this height do not trigger the normal scan (surface worlds stay cheap).
pub const UNDERGROUND_Y: f64 = 40.0;

/// The state a spawner currently presents to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Fighting,
    Ejecting,
    Cooldown,
}

impl Phase {
    fn block_state(self, player_near: bool) -> &'static str {
        match self {
            Self::Waiting if player_near => "waiting_for_players",
            Self::Waiting => "inactive",
            Self::Fighting => "active",
            Self::Ejecting => "waiting_for_reward_ejection",
            Self::Cooldown => "cooldown",
        }
    }
}

#[derive(Debug, Clone)]
struct Spawner {
    pos: [i32; 3],
    ominous: bool,
    phase: Phase,
    wave: u32,
    timer: u32,
    /// Every mob this trial spawned; an empty living set advances the trial.
    spawned: Vec<Uuid>,
}

impl Spawner {
    fn new(pos: [i32; 3], ominous: bool) -> Self {
        Self {
            pos,
            ominous,
            phase: Phase::Waiting,
            wave: 0,
            timer: 0,
            spawned: Vec::new(),
        }
    }
}

/// Region-owned trial spawners.
#[derive(Debug, Default)]
pub struct TrialSpawnerManager {
    spawners: Vec<Spawner>,
    scan_clock: u32,
    changed: Vec<[i32; 3]>,
}

impl TrialSpawnerManager {
    /// Takes the block positions whose state changed this tick, for block updates.
    pub fn take_changed(&mut self) -> Vec<[i32; 3]> {
        std::mem::take(&mut self.changed)
    }

    /// Moves the spawners whose block belongs to a split-off region.
    pub fn split(&mut self, take: impl Fn([i32; 3]) -> bool) -> Self {
        let (moving, staying) = std::mem::take(&mut self.spawners)
            .into_iter()
            .partition(|spawner| take(spawner.pos));
        self.spawners = staying;
        Self {
            spawners: moving,
            scan_clock: self.scan_clock,
            changed: Vec::new(),
        }
    }

    /// Absorbs the spawners of a merged region.
    pub fn merge(&mut self, mut other: Self) {
        self.spawners.append(&mut other.spawners);
    }

    /// Advances discovery and every live trial this region owns.
    pub fn tick(
        &mut self,
        store: &mut EntityStore,
        players: &mut HashMap<Uuid, Player>,
        rng: &mut Rng,
        blocks: &impl ferrite_entity::BlockGetter,
        source: Option<&AnvilChunkSource>,
    ) {
        self.scan_clock = self.scan_clock.wrapping_add(1);
        if self.scan_clock.is_multiple_of(SCAN_INTERVAL)
            && let Some(source) = source
        {
            self.discover(players, blocks, source);
        }
        self.advance(store, players, rng, blocks, source);
    }

    fn discover(
        &mut self,
        players: &mut HashMap<Uuid, Player>,
        blocks: &impl ferrite_entity::BlockGetter,
        source: &AnvilChunkSource,
    ) {
        let mut found: Vec<(Uuid, [i32; 3], bool)> = Vec::new();
        let known: HashSet<[i32; 3]> = self.spawners.iter().map(|spawner| spawner.pos).collect();
        for player in players.values() {
            if player.gamemode == 3 || player.survival.dead {
                continue;
            }
            let omen = player.effects.has(Effect::TrialOmen);
            if !omen && player.pos[1] > UNDERGROUND_Y {
                continue;
            }
            let cx = player.pos[0].floor() as i32;
            let cy = player.pos[1].floor() as i32;
            let cz = player.pos[2].floor() as i32;
            for x in cx - SCAN_RADIUS..=cx + SCAN_RADIUS {
                for y in cy - SCAN_Y..=cy + SCAN_Y {
                    for z in cz - SCAN_RADIUS..=cz + SCAN_RADIUS {
                        if !omen && known.contains(&[x, y, z]) {
                            continue;
                        }
                        let Some(state) = blocks.block_state(x, y, z) else {
                            continue;
                        };
                        let Some((block_ominous, state_name)) = block_info(state) else {
                            continue;
                        };
                        if state_name == "cooldown" {
                            continue;
                        }
                        found.push((player.uuid, [x, y, z], omen || block_ominous));
                    }
                }
            }
        }
        for (player, pos, ominous) in found {
            let already_known = self.spawners.iter().any(|spawner| spawner.pos == pos);
            if !already_known {
                self.spawners.push(Spawner::new(pos, ominous));
                write_state(source, pos, ominous, "waiting_for_players");
                self.changed.push(pos);
            }
            let Some(player) = players.get_mut(&player) else {
                continue;
            };
            if ominous && player.effects.has(Effect::TrialOmen) {
                player.remove_effect(Effect::TrialOmen);
            }
            if let Some(spawner) = self.spawners.iter_mut().find(|spawner| spawner.pos == pos)
                && ominous
                && !spawner.ominous
            {
                spawner.ominous = true;
                write_state(source, pos, true, spawner.phase.block_state(true));
                self.changed.push(pos);
            }
        }
    }

    fn advance(
        &mut self,
        store: &mut EntityStore,
        players: &HashMap<Uuid, Player>,
        rng: &mut Rng,
        blocks: &impl ferrite_entity::BlockGetter,
        source: Option<&AnvilChunkSource>,
    ) {
        let mut index = 0;
        while index < self.spawners.len() {
            let pos = self.spawners[index].pos;
            if blocks
                .block_state(pos[0], pos[1], pos[2])
                .and_then(block_info)
                .is_none()
            {
                self.spawners.swap_remove(index);
                continue;
            }
            let near = players.values().any(|player| {
                player.gamemode != 3
                    && !player.survival.dead
                    && (player.pos[0] - (f64::from(pos[0]) + 0.5)).abs() <= DETECTION_RANGE
                    && (player.pos[1] - (f64::from(pos[1]) + 0.5)).abs() <= DETECTION_RANGE
                    && (player.pos[2] - (f64::from(pos[2]) + 0.5)).abs() <= DETECTION_RANGE
            });
            let mut changed = false;
            {
                let spawner = &mut self.spawners[index];
                match spawner.phase {
                    Phase::Waiting => {
                        if spawner.timer > 0 {
                            spawner.timer -= 1;
                        } else if near {
                            spawn_wave(spawner, store, rng, blocks);
                            spawner.phase = Phase::Fighting;
                            changed = true;
                        }
                    }
                    Phase::Fighting => {
                        if !has_living_mob(store, &spawner.spawned) {
                            if spawner.wave + 1 < WAVES {
                                spawner.wave += 1;
                                spawner.timer = WAVE_DELAY_TICKS;
                                spawner.phase = Phase::Waiting;
                            } else {
                                spawner.phase = Phase::Ejecting;
                                spawner.timer = EJECT_DELAY_TICKS;
                            }
                            changed = true;
                        }
                    }
                    Phase::Ejecting => {
                        if spawner.timer > 0 {
                            spawner.timer -= 1;
                        } else {
                            eject_reward(spawner, store, rng);
                            spawner.phase = Phase::Cooldown;
                            spawner.timer = COOLDOWN_TICKS;
                            changed = true;
                        }
                    }
                    Phase::Cooldown => {
                        if spawner.timer > 0 {
                            spawner.timer -= 1;
                        } else {
                            spawner.phase = Phase::Waiting;
                            spawner.wave = 0;
                            spawner.spawned.clear();
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                let spawner = &self.spawners[index];
                if let Some(source) = source {
                    write_state(
                        source,
                        spawner.pos,
                        spawner.ominous,
                        spawner.phase.block_state(near),
                    );
                }
                self.changed.push(spawner.pos);
            }
            index += 1;
        }
    }
}

fn block_info(state: u16) -> Option<(bool, &'static str)> {
    let (name, props) = ferrite_data::blocks::state(state)?;
    if name != "minecraft:trial_spawner" {
        return None;
    }
    let ominous = props
        .iter()
        .any(|(key, value)| *key == "ominous" && *value == "true");
    let phase = props
        .iter()
        .find_map(|(key, value)| (*key == "trial_spawner_state").then_some(*value))
        .unwrap_or("inactive");
    Some((ominous, phase))
}

fn write_state(source: &AnvilChunkSource, pos: [i32; 3], ominous: bool, state: &str) {
    let Some(next) = ferrite_data::blocks::state_id(
        "minecraft:trial_spawner",
        &[
            ("ominous", if ominous { "true" } else { "false" }),
            ("trial_spawner_state", state),
        ],
    ) else {
        return;
    };
    if let Err(error) = source.set_block(pos[0], pos[1], pos[2], u32::from(next)) {
        tracing::warn!(?pos, %error, "could not update a trial spawner block");
    }
}

fn spawn_wave(
    spawner: &mut Spawner,
    store: &mut EntityStore,
    rng: &mut Rng,
    blocks: &impl ferrite_entity::BlockGetter,
) {
    let kinds = wave_kinds(spawner.ominous, spawner.wave);
    let count = kinds.len();
    for (index, kind) in kinds.into_iter().enumerate() {
        let angle = std::f64::consts::TAU * index as f64 / count as f64;
        let x = f64::from(spawner.pos[0]) + 0.5 + angle.cos() * 2.5;
        let z = f64::from(spawner.pos[2]) + 0.5 + angle.sin() * 2.5;
        let y = surface_y(
            blocks,
            x.floor() as i32,
            z.floor() as i32,
            spawner.pos[1] + 2,
        );
        let record = mob::new_mob(kind, Vec3::new(x, y, z), rng);
        spawner.spawned.push(record.data.uuid);
        store.insert(record);
    }
}

/// The trial mobs of one wave; the ominous trial uses the stronger trial-chamber set.
fn wave_kinds(ominous: bool, wave: u32) -> Vec<MobKind> {
    let set: [MobKind; 3] = if ominous {
        [MobKind::Husk, MobKind::Bogged, MobKind::Breeze]
    } else {
        [MobKind::Zombie, MobKind::Skeleton, MobKind::Spider]
    };
    let start = wave as usize % set.len();
    (0..MOBS_PER_WAVE)
        .map(|offset| set[(start + offset) % set.len()])
        .collect()
}

fn eject_reward(spawner: &Spawner, store: &mut EntityStore, rng: &mut Rng) {
    let table = if spawner.ominous {
        "minecraft:chests/trial_chambers/reward_ominous"
    } else {
        "minecraft:chests/trial_chambers/reward"
    };
    let context = ferrite_loot::LootContext::default();
    let mut stacks = super::drops::roll_table(table, &context).unwrap_or_default();
    if stacks.is_empty() {
        stacks.push((
            if spawner.ominous {
                "minecraft:ominous_trial_key".to_owned()
            } else {
                "minecraft:trial_key".to_owned()
            },
            1,
        ));
    }
    for (name, count) in stacks {
        let Some(stack) = ferrite_item::ItemStack::from_name(&name) else {
            continue;
        };
        let stack = stack.copy_with_count(count.max(1));
        let pos = Vec3::new(
            f64::from(spawner.pos[0]) + 0.5,
            f64::from(spawner.pos[1]) + 0.8,
            f64::from(spawner.pos[2]) + 0.5,
        );
        let vel = Vec3::new(rng.next_f64() * 0.2 - 0.1, 0.2, rng.next_f64() * 0.2 - 0.1);
        store.insert(ferrite_entity::item::new_item_stack(pos, vel, &stack, rng));
    }
}

fn has_living_mob(store: &EntityStore, spawned: &[Uuid]) -> bool {
    store.keys().iter().any(|&key| {
        let Some(data) = store.data(key) else {
            return false;
        };
        spawned.contains(&data.uuid)
            && store
                .flags(key)
                .is_none_or(|flags| !flags.has(Flags::REMOVED))
            && data.mob.is_some_and(|state| !state.is_dead())
    })
}

/// The highest block surface within a small vertical window, or the spawner's own height.
fn surface_y(blocks: &impl ferrite_entity::BlockGetter, x: i32, z: i32, around: i32) -> f64 {
    for y in (around - 8..=around + 2).rev() {
        let Some((name, _)) = blocks
            .block_state(x, y, z)
            .and_then(ferrite_data::blocks::state)
        else {
            continue;
        };
        let air = name == "minecraft:air"
            || name == "minecraft:cave_air"
            || name == "minecraft:void_air"
            || name.ends_with("_air");
        if !air {
            return f64::from(y + 1);
        }
    }
    f64::from(around)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ground;

    impl ferrite_entity::BlockGetter for Ground {
        fn block_state(&self, _x: i32, y: i32, _z: i32) -> Option<u16> {
            (y <= 63).then(|| ferrite_data::blocks::default_state("stone").unwrap())
        }
    }

    #[test]
    fn trial_spawner_state_is_recognised_from_its_block_state() {
        let state = ferrite_data::blocks::state_id(
            "minecraft:trial_spawner",
            &[
                ("ominous", "true"),
                ("trial_spawner_state", "waiting_for_reward_ejection"),
            ],
        )
        .unwrap();
        assert_eq!(
            block_info(state),
            Some((true, "waiting_for_reward_ejection"))
        );
        assert_eq!(
            block_info(ferrite_data::blocks::default_state("stone").unwrap()),
            None
        );
    }

    #[test]
    fn an_ominous_trial_uses_the_ominous_mob_set() {
        let normal = wave_kinds(false, 0);
        let ominous = wave_kinds(true, 0);
        assert_eq!(normal.len(), MOBS_PER_WAVE);
        assert_eq!(ominous.len(), MOBS_PER_WAVE);
        assert!(normal.contains(&MobKind::Zombie));
        assert!(ominous.contains(&MobKind::Bogged));
        assert!(wave_kinds(true, 1).contains(&MobKind::Breeze));
        assert_ne!(normal, ominous);
    }

    #[test]
    fn mobs_spawn_on_the_floor_above_the_spawner() {
        assert_eq!(surface_y(&Ground, 0, 0, 64), 64.0);
    }

    #[test]
    fn split_and_merge_keep_a_spawner_with_its_block() {
        let mut manager = TrialSpawnerManager::default();
        manager.spawners.push(Spawner::new([10, 20, 10], true));
        let other = manager.split(|pos| pos[0] >= 0);
        assert!(manager.spawners.is_empty());
        assert_eq!(other.spawners.len(), 1);
        manager.merge(other);
        assert_eq!(manager.spawners.len(), 1);
    }
}
