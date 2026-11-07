//! Block layouts of the end: the exit portal podium the dragon leaves behind and the end
//! gateways that lead to the outer islands.
//!
//! These are pure functions from a position to the blocks vanilla places; the fight state machine
//! and the entities live elsewhere.

use ferrite_data::blocks::{default_state, state_id};
use ferrite_entity::EntityData;
use ferrite_protocol::packets::play::MetaValue;

const BEDROCK: &str = "minecraft:bedrock";
const END_STONE: &str = "minecraft:end_stone";
const END_PORTAL: &str = "minecraft:end_portal";
const END_GATEWAY: &str = "minecraft:end_gateway";
const DRAGON_EGG: &str = "minecraft:dragon_egg";
const WALL_TORCH: &str = "minecraft:wall_torch";

/// Radius of the circle of end gateways around the main island, and their height.
pub const GATEWAY_RADIUS: f64 = 96.0;
pub const GATEWAY_Y: i32 = 75;
/// How many gateways a fight can open before the ring repeats.
pub const GATEWAY_COUNT: u32 = 20;

/// The four vanilla `DragonRespawnStage`s. `End` is represented by `respawn_stage: None`
/// after [`FightState::finish_respawn`] has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnStage {
    Start,
    PreparingToSummonPillars,
    SummoningPillars,
    SummoningDragon,
}

impl RespawnStage {
    /// Vanilla's serialized stage name (`DragonRespawnStage.getSerializedName`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::PreparingToSummonPillars => "preparing_to_summon_pillars",
            Self::SummoningPillars => "summoning_pillars",
            Self::SummoningDragon => "summoning_dragon",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "start" => Self::Start,
            "preparing_to_summon_pillars" => Self::PreparingToSummonPillars,
            "summoning_pillars" => Self::SummoningPillars,
            "summoning_dragon" => Self::SummoningDragon,
            _ => return None,
        })
    }
}

/// What the server remembers of the fight while it runs.
#[derive(Debug, Clone, Default)]
pub struct FightState {
    /// The dragon was spawned, or an earlier fight already ended with the exit portal.
    pub started: bool,
    /// A dragon already died in this world: later kills give less experience.
    pub killed: bool,
    /// Gateways opened so far; each kill opens the next one of the ring.
    pub gateways: u32,
    /// Spike numbers whose crystal was already placed (or that have none).
    pub crystals: std::collections::BTreeSet<u8>,
    /// A dragon is flying (spawned or respawned and not dead yet).
    pub dragon_alive: bool,
    /// The staged four-crystal respawn in flight, if any.
    pub respawn_stage: Option<RespawnStage>,
    /// Ticks spent in [`FightState::respawn_stage`], vanilla `respawnTime`.
    pub respawn_ticks: u32,
    /// The exit portal's y level, remembered after the portal is removed during the sequence.
    pub exit_portal_level: Option<i32>,
    /// The file was read (or found missing) already.
    pub loaded: bool,
    /// What the file holds right now, so only a change is written.
    pub on_disk: Option<Saved>,
}

/// The part of the fight that survives a restart (`ender_dragon_fight.dat`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub started: bool,
    pub killed: bool,
    pub dragon_alive: bool,
    pub gateways: u32,
    pub crystals: Vec<u8>,
    pub respawn_stage: Option<RespawnStage>,
    pub respawn_ticks: u32,
    pub exit_portal_level: Option<i32>,
}

impl Saved {
    pub fn to_nbt(&self) -> ferrite_nbt::Compound {
        use ferrite_nbt::Tag;
        let mut c = ferrite_nbt::Compound::new();
        c.insert("started".into(), Tag::Byte(i8::from(self.started)));
        c.insert("previously_killed".into(), Tag::Byte(i8::from(self.killed)));
        c.insert(
            "dragon_alive".into(),
            Tag::Byte(i8::from(self.dragon_alive)),
        );
        if let Some(stage) = self.respawn_stage {
            c.insert("respawn_stage".into(), Tag::String(stage.name().into()));
        }
        c.insert(
            "respawn_time".into(),
            Tag::Int(i32::try_from(self.respawn_ticks).unwrap_or(i32::MAX)),
        );
        if let Some(level) = self.exit_portal_level {
            c.insert("exit_portal_level".into(), Tag::Int(level));
        }
        c.insert(
            "gateways_opened".into(),
            Tag::Int(self.gateways.min(GATEWAY_COUNT) as i32),
        );
        c.insert(
            "crystals".into(),
            Tag::ByteArray(self.crystals.iter().map(|&n| n as i8).collect()),
        );
        c
    }

    pub fn from_nbt(c: &ferrite_nbt::Compound) -> Option<Self> {
        use ferrite_nbt::Tag;
        let flag = |key| matches!(c.get(key), Some(Tag::Byte(b)) if *b != 0);
        Some(Self {
            started: flag("started"),
            killed: flag("previously_killed"),
            dragon_alive: flag("dragon_alive"),
            respawn_stage: match c.get("respawn_stage") {
                Some(Tag::String(name)) => RespawnStage::from_name(name),
                _ => None,
            },
            respawn_ticks: match c.get("respawn_time") {
                Some(Tag::Int(ticks)) if *ticks >= 0 => *ticks as u32,
                _ => 0,
            },
            exit_portal_level: match c.get("exit_portal_level") {
                Some(Tag::Int(level)) => Some(*level),
                _ => None,
            },
            gateways: match c.get("gateways_opened") {
                Some(Tag::Int(n)) => u32::try_from(*n).ok()?.min(GATEWAY_COUNT),
                _ => 0,
            },
            crystals: match c.get("crystals") {
                Some(Tag::ByteArray(list)) => list.iter().map(|&n| n as u8).collect(),
                _ => Vec::new(),
            },
        })
    }
}

impl FightState {
    /// Starts vanilla's four-crystal respawn. The caller has already verified the layout.
    pub fn begin_respawn(&mut self, exit_portal_level: i32) -> bool {
        if !self.killed || self.dragon_alive || self.respawn_stage.is_some() {
            return false;
        }
        self.respawn_stage = Some(RespawnStage::Start);
        self.respawn_ticks = 0;
        self.exit_portal_level = Some(exit_portal_level);
        self.crystals.clear();
        true
    }

    /// Vanilla `abortRespawnSequence`: stop the animation without spawning a dragon.
    pub fn abort_respawn(&mut self) {
        self.respawn_stage = None;
        self.respawn_ticks = 0;
    }

    /// Vanilla `setRespawnStage(END)`: the dragon exists and the fight is running again.
    pub fn finish_respawn(&mut self) {
        self.respawn_stage = None;
        self.respawn_ticks = 0;
        self.dragon_alive = true;
    }

    /// Advances one vanilla stage tick. The returned actions are applied by the region; the state
    /// machine itself never spawns entities or sends packets.
    pub fn tick_respawn(&mut self) -> Vec<RespawnAction> {
        let Some(stage) = self.respawn_stage else {
            return Vec::new();
        };
        let time = self.respawn_ticks;
        let mut actions = Vec::new();
        match stage {
            RespawnStage::Start => {
                self.respawn_stage = Some(RespawnStage::PreparingToSummonPillars);
                self.respawn_ticks = 0;
                actions.push(RespawnAction::SetBeams(Some(RESPAWN_BEAM_TARGET)));
            }
            RespawnStage::PreparingToSummonPillars => {
                if time >= RESPAWN_PREPARING_TICKS {
                    self.respawn_stage = Some(RespawnStage::SummoningPillars);
                    self.respawn_ticks = 0;
                } else {
                    if matches!(time, 0 | 50 | 51 | 52 | 95) {
                        actions.push(RespawnAction::Roar);
                    }
                    self.respawn_ticks = time + 1;
                }
            }
            RespawnStage::SummoningPillars => {
                if time >= RESPAWN_PILLAR_TICKS {
                    self.respawn_stage = Some(RespawnStage::SummoningDragon);
                    self.respawn_ticks = 0;
                } else {
                    let index = (time / RESPAWN_PILLAR_INTERVAL) as u8;
                    if time.is_multiple_of(RESPAWN_PILLAR_INTERVAL) {
                        actions.push(RespawnAction::AimAtSpike { index });
                    } else if time % RESPAWN_PILLAR_INTERVAL == RESPAWN_PILLAR_INTERVAL - 1 {
                        actions.push(RespawnAction::RestoreSpike { index });
                    }
                    self.respawn_ticks = time + 1;
                }
            }
            RespawnStage::SummoningDragon => {
                if time >= RESPAWN_SUMMONING_TICKS {
                    self.respawn_stage = None;
                    self.respawn_ticks = 0;
                    actions.push(RespawnAction::SpawnDragon);
                } else {
                    if time == 0 {
                        actions.push(RespawnAction::SetBeams(Some(RESPAWN_BEAM_TARGET)));
                    } else if time < 5 || time == 80 {
                        actions.push(RespawnAction::Roar);
                    }
                    self.respawn_ticks = time + 1;
                }
            }
        }
        actions
    }

    /// Records and returns the next End gateway position. A fight opens each of the twenty
    /// vanilla gateways once; asking again after the ring is complete does not place duplicates.
    pub fn open_next_gateway(&mut self) -> Option<[i32; 3]> {
        if self.gateways >= GATEWAY_COUNT {
            self.gateways = GATEWAY_COUNT;
            return None;
        }
        let at = gateway_position(self.gateways);
        self.gateways += 1;
        Some(at)
    }

    pub fn saved(&self) -> Saved {
        Saved {
            started: self.started,
            killed: self.killed,
            dragon_alive: self.dragon_alive,
            gateways: self.gateways.min(GATEWAY_COUNT),
            crystals: self.crystals.iter().copied().collect(),
            respawn_stage: self.respawn_stage,
            respawn_ticks: self.respawn_ticks,
            exit_portal_level: self.exit_portal_level,
        }
    }

    pub fn restore(&mut self, saved: Saved) {
        self.started = saved.started;
        self.killed = saved.killed;
        self.dragon_alive = saved.dragon_alive;
        self.gateways = saved.gateways.min(GATEWAY_COUNT);
        self.crystals = saved.crystals.iter().copied().collect();
        self.respawn_stage = saved.respawn_stage;
        self.respawn_ticks = saved.respawn_ticks;
        self.exit_portal_level = saved.exit_portal_level;
        self.on_disk = Some(saved);
    }
}

/// How many obsidian spikes ring the main island, and how far from the centre they stand.
pub const SPIKE_COUNT: u8 = 10;
pub const SPIKE_RADIUS: f64 = 42.0;

/// The column of spike number `index`: ten evenly spaced around the world origin.
pub fn spike_column(index: u8) -> [i32; 2] {
    let angle = 2.0 * std::f64::consts::PI * f64::from(index) / f64::from(SPIKE_COUNT);
    [
        (SPIKE_RADIUS * angle.cos()).round() as i32,
        (SPIKE_RADIUS * angle.sin()).round() as i32,
    ]
}

/// The highest non-air block of a column, or `None` for a column that is all air (the void).
/// `block` answers the state at a height, or `None` while the chunk is not loaded.
pub fn highest_solid(mut block: impl FnMut(i32) -> Option<u32>) -> Option<i32> {
    for y in (0..=255).rev() {
        if block(y)? != 0 {
            return Some(y);
        }
    }
    None
}

/// The highest solid block of a column: where the exit podium or a spike top stands. An empty
/// loaded column falls back to y 63, the island's approximate surface; an unloaded chunk stays
/// `None` so the caller can retry.
pub fn podium_floor(block: impl Fn(i32) -> Option<u32>) -> Option<i32> {
    for y in (0..=255).rev() {
        if block(y)? != 0 {
            return Some(y);
        }
    }
    Some(63)
}

/// Squared distance of a block from the podium centre, as `Vec3i.closerThan` measures it.
fn distance_squared(origin: [i32; 3], at: [i32; 3]) -> f64 {
    let d = [
        f64::from(at[0] - origin[0]),
        f64::from(at[1] - origin[1]),
        f64::from(at[2] - origin[2]),
    ];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// After a kill the podium becomes the exit portal. The dragon egg appears only on the very
/// first kill of the world (vanilla `EnderDragonFight.dragonKilled`); later kills leave no egg.
pub fn dragon_death_blocks(origin: [i32; 3], first_kill: bool) -> Vec<([i32; 3], u16)> {
    let mut blocks = podium_blocks(origin, true);
    if first_kill && let Some(egg) = dragon_egg() {
        blocks.push((egg_position(origin), egg));
    }
    blocks
}

/// Whether the exit portal must be put back: the fight is over, no respawn is running, and the
/// portal block at the remembered level is gone (a death in another region, or a save taken
/// between the fight state and the block writes).
pub fn needs_exit_portal(
    killed: bool,
    dragon_alive: bool,
    respawn_stage: Option<RespawnStage>,
    portal_present: bool,
) -> bool {
    killed && !dragon_alive && respawn_stage.is_none() && !portal_present
}

/// Vanilla `EndPodiumFeature`: the bedrock fountain around `origin`. With `active` the middle
/// layer is end portal, otherwise air (the dragon is alive, or the portal is not open yet).
pub fn podium_blocks(origin: [i32; 3], active: bool) -> Vec<([i32; 3], u16)> {
    let (Some(bedrock), Some(stone), Some(portal)) = (
        default_state(BEDROCK),
        default_state(END_STONE),
        default_state(END_PORTAL),
    ) else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    for x in origin[0] - 4..=origin[0] + 4 {
        for y in origin[1] - 1..=origin[1] + 32 {
            for z in origin[2] - 4..=origin[2] + 4 {
                let at = [x, y, z];
                let d2 = distance_squared(origin, at);
                let close = d2 < 2.5 * 2.5;
                if !(close || d2 < 3.5 * 3.5) {
                    continue;
                }
                let state = if y < origin[1] {
                    if close { bedrock } else { stone }
                } else if y > origin[1] {
                    0
                } else if !close {
                    bedrock
                } else if active {
                    portal
                } else {
                    0
                };
                blocks.push((at, state));
            }
        }
    }
    // The bedrock column in the middle, with a torch on each side of its third block.
    for up in 0..4 {
        blocks.push(([origin[0], origin[1] + up, origin[2]], bedrock));
    }
    for (dx, dz, facing) in [
        (0, -1, "north"),
        (0, 1, "south"),
        (-1, 0, "west"),
        (1, 0, "east"),
    ] {
        if let Some(torch) = state_id(WALL_TORCH, &[("facing", facing)]) {
            blocks.push(([origin[0] + dx, origin[1] + 2, origin[2] + dz], torch));
        }
    }
    blocks
}

/// The four cells next to the exit portal where end crystals bring the dragon back
/// (`EnderDragonFight.tryRespawn`: three blocks from the centre, one above the portal level).
pub fn respawn_crystal_cells(level: i32) -> [[i32; 3]; 4] {
    [
        [3, level + 1, 0],
        [-3, level + 1, 0],
        [0, level + 1, 3],
        [0, level + 1, -3],
    ]
}

/// Vanilla `DragonRespawnStage` timings: 100 ticks preparing, one spike every 40 ticks for ten
/// spikes, then 100 ticks summoning the dragon.
pub const RESPAWN_PREPARING_TICKS: u32 = 100;
pub const RESPAWN_PILLAR_INTERVAL: u32 = 40;
pub const RESPAWN_PILLAR_TICKS: u32 = 400;
pub const RESPAWN_SUMMONING_TICKS: u32 = 100;
/// The beam target used by `START` and `SUMMONING_DRAGON` (vanilla `BlockPos(0, 128, 0)`).
pub const RESPAWN_BEAM_TARGET: [i32; 3] = [0, 128, 0];
/// Vanilla `LevelEvent.ANIMATION_DRAGON_SUMMON_ROAR` in 26.3.
pub const DRAGON_SUMMON_ROAR_EVENT: i32 = 3001;
/// `EndCrystal` synched data: optional beam target and whether the bottom is drawn.
pub const END_CRYSTAL_BEAM_TARGET: u8 = 8;
pub const END_CRYSTAL_SHOW_BOTTOM: u8 = 9;

/// One action for the region to perform after a respawn stage tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnAction {
    SetBeams(Option<[i32; 3]>),
    Roar,
    AimAtSpike { index: u8 },
    RestoreSpike { index: u8 },
    SpawnDragon,
}

/// Picks one crystal from each of the four required cells. `None` means the layout is invalid
/// and `tryRespawn` must not start.
pub fn require_respawn_crystals<T>(
    cells: [[i32; 3]; 4],
    mut crystal_at: impl FnMut([i32; 3]) -> Option<T>,
) -> Option<[T; 4]> {
    Some([
        crystal_at(cells[0])?,
        crystal_at(cells[1])?,
        crystal_at(cells[2])?,
        crystal_at(cells[3])?,
    ])
}

/// Sets an End crystal's beam target (and the placed-crystal `show_bottom = false`), mirroring
/// vanilla `EndCrystal.setBeamTarget` + `EndCrystalItem.useOn`.
pub fn set_crystal_beam(data: &mut EntityData, target: Option<[i32; 3]>) {
    if data.metadata.get(END_CRYSTAL_BEAM_TARGET).is_none() {
        data.metadata
            .define(END_CRYSTAL_BEAM_TARGET, MetaValue::OptionalBlockPos(None));
    }
    if data.metadata.get(END_CRYSTAL_SHOW_BOTTOM).is_none() {
        data.metadata
            .define(END_CRYSTAL_SHOW_BOTTOM, MetaValue::Boolean(true));
    }
    data.metadata
        .set(END_CRYSTAL_SHOW_BOTTOM, MetaValue::Boolean(false));
    data.metadata.set(
        END_CRYSTAL_BEAM_TARGET,
        MetaValue::OptionalBlockPos(
            target.map(|[x, y, z]| ferrite_protocol::types::BlockPos::new(x, y, z).pack()),
        ),
    );
}

/// Where the dragon egg appears once the dragon dies.
pub fn egg_position(origin: [i32; 3]) -> [i32; 3] {
    [origin[0], origin[1] + 4, origin[2]]
}

pub fn dragon_egg() -> Option<u16> {
    default_state(DRAGON_EGG)
}

/// How far around the ideal gateway exit to look for the outer island.
pub const GATEWAY_LANDING_RADIUS: i32 = 16;

/// Where a player stepping into a gateway lands: the nearest loaded column with ground and two
/// blocks of air above it, searched outward from the point 1024 blocks out. Vanilla searches
/// the nearest outer island; the ideal point can be over the void between islands. `None`
/// means no loaded column offered a safe landing.
pub fn gateway_landing(
    [center_x, center_z]: [i32; 2],
    mut block: impl FnMut(i32, i32, i32) -> Option<u32>,
) -> Option<[f64; 3]> {
    for radius in 0..=GATEWAY_LANDING_RADIUS {
        let mut best: Option<([i32; 3], i32)> = None;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if radius > 0 && dx.abs() != radius && dz.abs() != radius {
                    continue;
                }
                let (x, z) = (center_x + dx, center_z + dz);
                let Some(top) = highest_solid(|y| block(x, y, z)) else {
                    continue;
                };
                if !(1..=253).contains(&top)
                    || block(x, top + 1, z) != Some(0)
                    || block(x, top + 2, z) != Some(0)
                {
                    continue;
                }
                let distance = dx * dx + dz * dz;
                if best.is_none_or(|(_, nearest)| distance < nearest) {
                    best = Some(([x, top, z], distance));
                }
            }
        }
        if let Some(([x, top, z], _)) = best {
            return Some([f64::from(x) + 0.5, f64::from(top) + 1.0, f64::from(z) + 0.5]);
        }
    }
    None
}

/// The position of gateway number `index` (0-based, wrapping at twenty): vanilla spaces them
/// evenly on a circle of radius 96 around the world origin at y 75.
pub fn gateway_position(index: u32) -> [i32; 3] {
    let angle = 2.0
        * (-std::f64::consts::PI + std::f64::consts::PI / 20.0 * f64::from(index % GATEWAY_COUNT));
    [
        (GATEWAY_RADIUS * angle.cos()).floor() as i32,
        GATEWAY_Y,
        (GATEWAY_RADIUS * angle.sin()).floor() as i32,
    ]
}

/// Vanilla `EndGatewayFeature`: the gateway block in a bedrock cross, with air around it.
pub fn gateway_blocks(origin: [i32; 3]) -> Vec<([i32; 3], u16)> {
    let (Some(bedrock), Some(gateway)) = (default_state(BEDROCK), default_state(END_GATEWAY))
    else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    for x in origin[0] - 1..=origin[0] + 1 {
        for y in origin[1] - 2..=origin[1] + 2 {
            for z in origin[2] - 1..=origin[2] + 1 {
                let same_x = x == origin[0];
                let same_y = y == origin[1];
                let same_z = z == origin[2];
                let not_center_y = (y - origin[1]).abs() == 2;
                let state = if same_x && same_y && same_z {
                    gateway
                } else if same_y {
                    0
                } else if (not_center_y && same_x && same_z)
                    || ((same_x || same_z) && !not_center_y)
                {
                    bedrock
                } else {
                    0
                };
                blocks.push(([x, y, z], state));
            }
        }
    }
    blocks
}

/// Experience the dragon drops: 12000 for the first kill of a fight world, 500 afterwards.
pub fn dragon_experience(first_kill: bool) -> u32 {
    if first_kill { 12_000 } else { 500 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn layout(blocks: Vec<([i32; 3], u16)>) -> HashMap<[i32; 3], u16> {
        blocks.into_iter().collect()
    }

    fn named(state: u16) -> &'static str {
        ferrite_data::blocks::state(state).map_or("?", |(name, _)| name)
    }

    #[test]
    fn the_podium_is_a_bedrock_fountain_with_a_portal_in_its_middle() {
        let origin = [0, 64, 0];
        let active = layout(podium_blocks(origin, true));
        assert_eq!(
            named(active[&[0, 64, 0]]),
            BEDROCK,
            "the pillar stands in the middle"
        );
        assert_eq!(named(active[&[2, 64, 0]]), END_PORTAL);
        assert_eq!(named(active[&[1, 64, 1]]), END_PORTAL);
        assert_eq!(
            named(active[&[3, 64, 0]]),
            BEDROCK,
            "the rim of the top layer"
        );
        assert_eq!(named(active[&[0, 63, 0]]), BEDROCK, "under the portal");
        assert_eq!(
            named(active[&[3, 63, 0]]),
            END_STONE,
            "the outer ring below"
        );
        assert_eq!(active[&[3, 65, 0]], 0, "air above the rim");
        assert_eq!(named(active[&[0, 66, 0]]), BEDROCK, "the central column");
        assert_eq!(named(active[&[0, 67, 0]]), BEDROCK);
        assert_eq!(named(active[&[1, 66, 0]]), WALL_TORCH);
        let dormant = layout(podium_blocks(origin, false));
        assert_eq!(dormant[&[2, 64, 0]], 0, "no portal while the dragon lives");
        assert_eq!(named(dormant[&[3, 64, 0]]), BEDROCK);
    }

    #[test]
    fn twenty_gateways_sit_on_a_circle_of_ninety_six_blocks() {
        let all: Vec<[i32; 3]> = (0..GATEWAY_COUNT).map(gateway_position).collect();
        assert_eq!(all[0], [96, 75, 0], "the ring starts to the east");
        for position in &all {
            let r = f64::from(position[0]).hypot(f64::from(position[2]));
            assert!(
                (r - GATEWAY_RADIUS).abs() < 1.5,
                "{position:?} is {r} from the centre"
            );
            assert_eq!(position[1], GATEWAY_Y);
        }
        assert_eq!(gateway_position(GATEWAY_COUNT), all[0], "the ring wraps");
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), 20);
    }

    #[test]
    fn a_gateway_is_a_bedrock_cross_around_the_portal_block() {
        let origin = [10, 75, 20];
        let blocks = layout(gateway_blocks(origin));
        assert_eq!(named(blocks[&origin]), END_GATEWAY);
        assert_eq!(named(blocks[&[10, 73, 20]]), BEDROCK, "below");
        assert_eq!(named(blocks[&[10, 77, 20]]), BEDROCK, "above");
        assert_eq!(
            named(blocks[&[10, 74, 20]]),
            BEDROCK,
            "the cross arm under the block"
        );
        assert_eq!(blocks[&[11, 75, 20]], 0, "air beside it");
        assert_eq!(blocks[&[11, 74, 21]], 0, "air in the corners");
        assert_eq!(blocks.len(), 3 * 5 * 3);
    }

    #[test]
    fn ten_spikes_ring_the_island_at_forty_two_blocks() {
        let columns: Vec<_> = (0..SPIKE_COUNT).map(spike_column).collect();
        assert_eq!(columns[0], [42, 0]);
        assert_eq!(columns[5], [-42, 0]);
        for c in &columns {
            let r = f64::from(c[0]).hypot(f64::from(c[1]));
            assert!((r - SPIKE_RADIUS).abs() < 1.0, "{c:?}");
        }
        assert_eq!(podium_floor(|y| Some(u32::from(y <= 60))), Some(60));
        assert_eq!(
            podium_floor(|_| None),
            None,
            "an unloaded chunk has no floor yet"
        );
    }

    #[test]
    fn the_egg_sits_four_blocks_above_the_podium_floor() {
        assert_eq!(egg_position([0, 64, 0]), [0, 68, 0]);
        assert_eq!(named(dragon_egg().unwrap()), DRAGON_EGG);
        assert_eq!(dragon_experience(true), 12_000);
        assert_eq!(dragon_experience(false), 500);
    }

    #[test]
    fn only_the_first_kill_leaves_a_dragon_egg() {
        let first = layout(dragon_death_blocks([0, 64, 0], true));
        assert_eq!(named(first[&[2, 64, 0]]), END_PORTAL);
        assert_eq!(named(first[&[0, 68, 0]]), DRAGON_EGG);
        let later = layout(dragon_death_blocks([0, 64, 0], false));
        assert_eq!(named(later[&[2, 64, 0]]), END_PORTAL);
        assert_eq!(later.get(&[0, 68, 0]), None, "the egg is not duplicated");
    }

    #[test]
    fn a_void_column_has_no_top_but_the_podium_still_has_a_floor() {
        assert_eq!(highest_solid(|_| Some(0)), None);
        assert_eq!(podium_floor(|_| Some(0)), Some(63));
        assert_eq!(highest_solid(|y| Some(u32::from(y <= 70))), Some(70));
    }

    #[test]
    fn a_gateway_landing_finds_the_nearest_ground_and_skips_the_void() {
        let mut ground = HashMap::new();
        ground.insert((5, 0), 64);
        ground.insert((-4, 4), 80);
        // The nearer column is full to the build height, so there is no room to stand.
        ground.insert((2, 0), 255);
        let mut block = |x: i32, y: i32, z: i32| -> Option<u32> {
            match ground.get(&(x, z)) {
                Some(&top) if y == top => Some(1),
                Some(&top) if y > top => Some(0),
                _ => Some(0),
            }
        };
        assert_eq!(
            gateway_landing([0, 0], &mut block),
            Some([-3.5, 81.0, 4.5]),
            "the column without room and the farther one are skipped"
        );
        let mut void = |_: i32, _: i32, _: i32| Some(0);
        assert_eq!(gateway_landing([0, 0], &mut void), None);
    }

    #[test]
    fn the_exit_portal_is_repaired_only_after_the_dragon_is_gone() {
        assert!(needs_exit_portal(true, false, None, false));
        assert!(!needs_exit_portal(true, false, None, true));
        assert!(!needs_exit_portal(false, false, None, false));
        assert!(!needs_exit_portal(true, true, None, false));
        assert!(!needs_exit_portal(
            true,
            false,
            Some(RespawnStage::SummoningPillars),
            false
        ));
    }
}

#[cfg(test)]
mod respawn_tests {
    use super::*;

    #[test]
    fn the_respawn_crystals_stand_on_the_bedrock_ring_of_the_podium() {
        let level = 60;
        let ring: std::collections::HashSet<[i32; 3]> = podium_blocks([0, level, 0], true)
            .into_iter()
            .filter(|(_, state)| Some(*state) == default_state(BEDROCK))
            .map(|(at, _)| at)
            .collect();
        for [x, y, z] in respawn_crystal_cells(level) {
            assert!(ring.contains(&[x, y - 1, z]), "bedrock under {x} {y} {z}");
        }
    }

    #[test]
    fn the_fight_survives_a_round_trip_through_nbt() {
        let mut fight = FightState {
            started: true,
            killed: true,
            gateways: 3,
            respawn_stage: Some(RespawnStage::SummoningPillars),
            respawn_ticks: 39,
            exit_portal_level: Some(64),
            ..Default::default()
        };
        fight.crystals.extend([0, 4, 9]);
        let back = Saved::from_nbt(&fight.saved().to_nbt()).unwrap();
        assert_eq!(back, fight.saved());
        let mut fresh = FightState::default();
        fresh.restore(back);
        assert!(fresh.started && fresh.killed && fresh.crystals.len() == 3);
        assert_eq!(fresh.respawn_stage, Some(RespawnStage::SummoningPillars));
        assert_eq!(fresh.respawn_ticks, 39);
        assert_eq!(fresh.exit_portal_level, Some(64));
    }

    #[test]
    fn the_respawn_layout_requires_all_four_cells() {
        let cells = respawn_crystal_cells(64);
        assert_eq!(require_respawn_crystals(cells, Some), Some(cells));
        for missing in 0..4 {
            assert_eq!(
                require_respawn_crystals(cells, |cell| (cell != cells[missing]).then_some(cell)),
                None,
                "missing cell {missing} must not start the sequence"
            );
        }
    }

    #[test]
    fn the_respawn_stage_matches_vanilla_delay_and_event_order() {
        let mut fight = FightState {
            started: true,
            killed: true,
            ..Default::default()
        };
        assert!(fight.begin_respawn(64));
        let mut events = Vec::new();
        for tick in 1..=700 {
            for action in fight.tick_respawn() {
                events.push((tick, action));
            }
            if fight.respawn_stage.is_none() {
                break;
            }
        }
        let roars: Vec<u32> = events
            .iter()
            .filter(|(_, action)| *action == RespawnAction::Roar)
            .map(|(tick, _)| *tick)
            .collect();
        assert_eq!(roars, vec![2, 52, 53, 54, 97, 505, 506, 507, 508, 584]);
        let aimed: Vec<u8> = events
            .iter()
            .filter_map(|(_, action)| match action {
                RespawnAction::AimAtSpike { index } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(aimed, (0..SPIKE_COUNT).collect::<Vec<_>>());
        let restored: Vec<u8> = events
            .iter()
            .filter_map(|(_, action)| match action {
                RespawnAction::RestoreSpike { index } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(restored, (0..SPIKE_COUNT).collect::<Vec<_>>());
        let spawns: Vec<u32> = events
            .iter()
            .filter_map(|(tick, action)| (*action == RespawnAction::SpawnDragon).then_some(*tick))
            .collect();
        assert_eq!(spawns, vec![604], "one dragon after the vanilla delay");
        assert_eq!(
            events
                .iter()
                .filter(|(_, action)| {
                    *action == RespawnAction::SetBeams(Some(RESPAWN_BEAM_TARGET))
                })
                .map(|(tick, _)| *tick)
                .collect::<Vec<_>>(),
            vec![1, 504]
        );
        assert!(fight.tick_respawn().is_empty());
        fight.finish_respawn();
        assert!(fight.dragon_alive);
        assert!(fight.tick_respawn().is_empty(), "the spawn is not repeated");
    }

    #[test]
    fn an_interrupted_respawn_aborts_and_can_start_again() {
        let mut fight = FightState {
            started: true,
            killed: true,
            ..Default::default()
        };
        assert!(fight.begin_respawn(64));
        for _ in 0..50 {
            fight.tick_respawn();
        }
        assert_eq!(
            fight.respawn_stage,
            Some(RespawnStage::PreparingToSummonPillars)
        );
        fight.abort_respawn();
        assert_eq!(fight.respawn_stage, None);
        assert_eq!(fight.respawn_ticks, 0);
        assert!(fight.tick_respawn().is_empty());
        assert!(fight.begin_respawn(64), "the sequence can be retried");
        assert_eq!(fight.respawn_stage, Some(RespawnStage::Start));
    }

    #[test]
    fn a_saved_respawn_resumes_at_its_stage() {
        let mut fight = FightState {
            started: true,
            killed: true,
            gateways: 7,
            ..Default::default()
        };
        assert!(fight.begin_respawn(64));
        for _ in 0..102 {
            fight.tick_respawn();
        }
        assert_eq!(fight.respawn_stage, Some(RespawnStage::SummoningPillars));
        assert_eq!(fight.respawn_ticks, 0);
        let saved = fight.saved();
        let mut resumed = FightState::default();
        resumed.restore(saved.clone());
        assert_eq!(resumed.saved(), saved);
        assert_eq!(resumed.respawn_stage, Some(RespawnStage::SummoningPillars));
        assert_eq!(resumed.exit_portal_level, Some(64));
        assert_eq!(resumed.gateways, 7);
        assert!(resumed.killed && !resumed.dragon_alive);
        assert!(matches!(
            resumed.tick_respawn().as_slice(),
            [RespawnAction::AimAtSpike { index: 0 }]
        ));
    }

    #[test]
    fn respawn_crystals_show_a_beam_and_hide_their_bottom() {
        let mut crystal =
            ferrite_entity::dragon::new_crystal(ferrite_math::Vec3::new(0.5, 65.0, 0.5));
        set_crystal_beam(&mut crystal.data, Some([0, 128, 0]));
        assert_eq!(
            crystal.data.metadata.get(END_CRYSTAL_BEAM_TARGET),
            Some(&MetaValue::OptionalBlockPos(Some(
                ferrite_protocol::types::BlockPos::new(0, 128, 0).pack()
            )))
        );
        assert_eq!(
            crystal.data.metadata.get(END_CRYSTAL_SHOW_BOTTOM),
            Some(&MetaValue::Boolean(false))
        );
        set_crystal_beam(&mut crystal.data, None);
        assert_eq!(
            crystal.data.metadata.get(END_CRYSTAL_BEAM_TARGET),
            Some(&MetaValue::OptionalBlockPos(None))
        );
    }

    #[test]
    fn fight_opens_each_gateway_once_and_stops_after_the_ring() {
        let mut fight = FightState {
            gateways: GATEWAY_COUNT - 1,
            ..Default::default()
        };
        let last = fight.open_next_gateway();
        assert_eq!(last, Some(gateway_position(GATEWAY_COUNT - 1)));
        assert_eq!(fight.gateways, GATEWAY_COUNT);
        assert_eq!(fight.open_next_gateway(), None);
        assert_eq!(fight.gateways, GATEWAY_COUNT);

        let oversized = Saved {
            started: true,
            killed: true,
            dragon_alive: false,
            gateways: u32::MAX,
            crystals: Vec::new(),
            respawn_stage: None,
            respawn_ticks: 0,
            exit_portal_level: None,
        };
        let restored = Saved::from_nbt(&oversized.to_nbt()).unwrap();
        assert_eq!(restored.gateways, GATEWAY_COUNT);
    }
}
