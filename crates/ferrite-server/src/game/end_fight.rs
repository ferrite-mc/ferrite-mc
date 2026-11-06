//! Block layouts of the end: the exit portal podium the dragon leaves behind and the end
//! gateways that lead to the outer islands.
//!
//! These are pure functions from a position to the blocks vanilla places; the fight state machine
//! and the entities live elsewhere.

use ferrite_data::blocks::{default_state, state_id};

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
        c.insert("gateways_opened".into(), Tag::Int(self.gateways as i32));
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
            gateways: match c.get("gateways_opened") {
                Some(Tag::Int(n)) => u32::try_from(*n).ok()?,
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
    pub fn saved(&self) -> Saved {
        Saved {
            started: self.started,
            killed: self.killed,
            dragon_alive: self.dragon_alive,
            gateways: self.gateways,
            crystals: self.crystals.iter().copied().collect(),
        }
    }

    pub fn restore(&mut self, saved: Saved) {
        self.started = saved.started;
        self.killed = saved.killed;
        self.dragon_alive = saved.dragon_alive;
        self.gateways = saved.gateways;
        self.crystals = saved.crystals.iter().copied().collect();
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

/// The highest solid block of a column: where the exit podium or a spike top stands. `block`
/// answers the state at a height, or `None` while the chunk is not loaded.
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

/// Where the dragon egg appears once the dragon dies.
pub fn egg_position(origin: [i32; 3]) -> [i32; 3] {
    [origin[0], origin[1] + 4, origin[2]]
}

pub fn dragon_egg() -> Option<u16> {
    default_state(DRAGON_EGG)
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
            ..Default::default()
        };
        fight.crystals.extend([0, 4, 9]);
        let back = Saved::from_nbt(&fight.saved().to_nbt()).unwrap();
        assert_eq!(back, fight.saved());
        let mut fresh = FightState::default();
        fresh.restore(back);
        assert!(fresh.started && fresh.killed && fresh.crystals.len() == 3);
    }
}
