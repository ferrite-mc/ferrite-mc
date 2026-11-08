//! The ender dragon's flight: a slow circle, a dive at nearby players, a fireball, and a
//! landing cycle around the exit portal.
//!
//! The circle is still the holding pattern of vanilla's phase machine; the landing decision and
//! its approach/perch/takeoff phases are modelled explicitly, while strafing remains simplified.

use crate::ai::{MobEvent, Target};
use crate::id::{EntityKey, allocate_id, random_uuid};
use crate::mob;
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use ferrite_math::{AABB, Vec3};
use uuid::Uuid;

/// Centre of the circle: the end's world origin.
pub const CENTER: [f64; 2] = [0.0, 0.0];
/// Radius of the circle in blocks.
pub const RADIUS: f64 = 40.0;
/// Average height of the circle and how far the dragon bobs around it.
pub const HEIGHT: f64 = 90.0;
pub const BOB: f64 = 6.0;
/// Angular speed in radians per tick (about 0.6 blocks per tick at the radius above).
pub const ANGULAR_SPEED: f64 = 0.015;

/// Where the dragon is `age` ticks after it started to fly, and the yaw it faces.
pub fn position_at(age: u32) -> (Vec3, f32) {
    let angle = f64::from(age) * ANGULAR_SPEED;
    let pos = Vec3::new(
        CENTER[0] + RADIUS * angle.cos(),
        HEIGHT + BOB * (f64::from(age) * 0.05).sin(),
        CENTER[1] + RADIUS * angle.sin(),
    );
    // Tangent of the circle: d/dangle = (-sin, cos).
    let (dx, dz) = (-angle.sin(), angle.cos());
    let yaw = (-dx).atan2(dz).to_degrees() as f32;
    (pos, yaw)
}

/// The dragon never moves faster than this many blocks per tick.
pub const MAX_SPEED: f64 = 1.2;
/// A dive at the nearest player starts every `CHARGE_PERIOD` ticks, lasts `CHARGE_TICKS` and
/// only happens for players within `CHARGE_RANGE` blocks.
pub const CHARGE_PERIOD: u32 = 600;
pub const CHARGE_TICKS: u32 = 80;
pub const CHARGE_RANGE: f64 = 100.0;
/// The dragon stops and faces its target for this many ticks before diving.
pub const CHARGE_TELEGRAPH_TICKS: u32 = 20;
/// Players within this range can be tracked as the dragon's current combat target.
pub const TARGET_RANGE: f64 = 128.0;
/// Players closer than this to the dragon's body are hit (wings and body, `knockBack`).
pub const CONTACT_RANGE: f64 = 5.0;
/// Damage of one hit and the ticks between hits.
pub const CONTACT_DAMAGE: f32 = 6.0;
pub const CONTACT_COOLDOWN: u16 = 20;

/// Combat phases are derived from persisted dragon age, so a save during a charge does not replay
/// the telegraph or restart its attack timer. Landing/perching is stored separately in
/// [`DragonPhase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Circling,
    Telegraphing,
    Charging,
}

impl Phase {
    pub fn at(age: u32, has_target: bool) -> Self {
        if !has_target {
            return Self::Circling;
        }
        let cycle = age % CHARGE_PERIOD;
        let charge_start = CHARGE_PERIOD - CHARGE_TICKS;
        if cycle < charge_start {
            Self::Circling
        } else if cycle < charge_start + CHARGE_TELEGRAPH_TICKS {
            Self::Telegraphing
        } else {
            Self::Charging
        }
    }
}
/// The dragon's landing cycle. Vanilla chooses this when a holding-pattern path ends; Ferrite
/// uses a fixed-length holding window so the decision is deterministic and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragonPhase {
    #[default]
    Holding,
    LandingApproach,
    Perched,
    Takeoff,
}

impl DragonPhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Holding => "holding",
            Self::LandingApproach => "landing_approach",
            Self::Perched => "perched",
            Self::Takeoff => "takeoff",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "holding" => Some(Self::Holding),
            "landing_approach" => Some(Self::LandingApproach),
            "perched" => Some(Self::Perched),
            "takeoff" => Some(Self::Takeoff),
            _ => None,
        }
    }
}

/// Ticks of circling before the dragon decides whether to land. Vanilla makes that decision
/// when a path ends; this window keeps the decision deterministic without modelling the path.
pub const HOLDING_DECISION_TICKS: u32 = 1200;
/// The dragon starts its approach after this many holding-window ticks when the roll succeeds,
/// with a chance of `1 / (alive crystals + 3)`.
pub fn should_land(alive_crystals: u32, roll: u32) -> bool {
    roll.is_multiple_of(alive_crystals.saturating_add(3))
}
/// The distance to the perch spot at which the approach becomes a perch.
pub const PERCH_REACH: f64 = 4.0;
/// How long the dragon stays perched before taking off again.
pub const PERCH_TICKS: u32 = 200;
pub const PERCH_SPOT: Vec3 = Vec3::new(0.0, 68.0, 0.0);
pub const BREATH_INTERVAL: u32 = 20;
/// The distance to the circling path at which takeoff ends.
pub const TAKEOFF_REACH: f64 = 4.0;
/// A dragon fireball's cloud starts at radius 3, waits 20 ticks, then grows to radius 5 over its
/// 30-second life (vanilla `DragonFireball`). The perched breath uses the same cloud shape with
/// no initial wait. The harmful part hurts for 3 every 20 ticks, 3 health per second.
pub const BREATH_TICKS: u32 = 600;
pub const BREATH_RADIUS: f64 = 3.0;
pub const BREATH_MAX_RADIUS: f64 = 5.0;
pub const BREATH_WAIT_TICKS: u32 = 20;
pub const BREATH_HIT_INTERVAL: u32 = 20;
pub const BREATH_DAMAGE: f32 = 3.0;

/// A vanilla `AreaEffectCloud` of dragon's breath. It remains visible for its whole duration;
/// only after `wait` does the cloud start hurting players inside its growing radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreathCloud {
    pub center: Vec3,
    pub age: u32,
    pub duration: u32,
    pub wait: u32,
    pub initial_radius: f64,
    pub max_radius: f64,
}

impl BreathCloud {
    pub fn fireball(center: Vec3) -> Self {
        Self {
            center,
            age: 0,
            duration: BREATH_TICKS,
            wait: BREATH_WAIT_TICKS,
            initial_radius: BREATH_RADIUS,
            max_radius: BREATH_MAX_RADIUS,
        }
    }

    pub fn perched(center: Vec3) -> Self {
        Self {
            center,
            age: 0,
            duration: BREATH_TICKS,
            wait: 0,
            initial_radius: BREATH_RADIUS,
            max_radius: BREATH_MAX_RADIUS,
        }
    }

    /// The visible and harmful radius at the current age.
    pub fn radius(&self) -> f64 {
        let progress = f64::from(self.age.min(self.duration)) / f64::from(self.duration.max(1));
        self.initial_radius + (self.max_radius - self.initial_radius) * progress
    }

    /// Advances one tick and returns the damage to apply this tick (0 while waiting or between
    /// hit intervals). `None` means the cloud has expired and must be removed.
    pub fn advance(&mut self) -> Option<f32> {
        if self.age >= self.duration {
            return None;
        }
        self.age += 1;
        if self.age <= self.wait || !self.age.is_multiple_of(BREATH_HIT_INTERVAL) {
            return Some(0.0);
        }
        Some(BREATH_DAMAGE)
    }
}

/// The eight damageable vanilla sub-hitboxes of the dragon. Their boxes are axis-aligned and
/// move with the dragon; they do not rotate with its yaw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragonPart {
    Head,
    Neck,
    Body,
    Tail,
    Wing,
}

pub const DRAGON_PART_COUNT: usize = 8;

/// Vanilla hitboxes: head 1x1, neck 3x3, body 3 high x 5 wide, three 2x2 tail parts and two
/// 2 high x 4 wide wings. Local offsets approximate the vanilla model around the entity feet.
pub fn part_boxes(pos: Vec3, yaw: f32) -> [(DragonPart, AABB); DRAGON_PART_COUNT] {
    let angle = f64::from(yaw).to_radians();
    let (sin, cos) = angle.sin_cos();
    let forward = Vec3::new(-sin, 0.0, cos);
    let right = Vec3::new(cos, 0.0, sin);
    let part = |part_forward: f64, part_right: f64, height: f64, width: f64, tall: f64| {
        let center = pos + forward * part_forward + right * part_right;
        let center = Vec3::new(center.x, pos.y + height, center.z);
        AABB::new(
            center.x - width * 0.5,
            center.y - tall * 0.5,
            center.z - width * 0.5,
            center.x + width * 0.5,
            center.y + tall * 0.5,
            center.z + width * 0.5,
        )
    };
    [
        (DragonPart::Head, part(5.0, 0.0, 3.0, 1.0, 1.0)),
        (DragonPart::Neck, part(3.0, 0.0, 3.0, 3.0, 3.0)),
        (DragonPart::Body, part(0.0, 0.0, 3.0, 5.0, 3.0)),
        (DragonPart::Tail, part(-3.0, 0.0, 2.0, 2.0, 2.0)),
        (DragonPart::Tail, part(-5.0, 0.0, 2.0, 2.0, 2.0)),
        (DragonPart::Tail, part(-7.0, 0.0, 2.0, 2.0, 2.0)),
        (DragonPart::Wing, part(0.0, 4.0, 4.0, 4.0, 2.0)),
        (DragonPart::Wing, part(0.0, -4.0, 4.0, 4.0, 2.0)),
    ]
}

/// Vanilla `EnderDragon.hurt`: only the head takes full damage. Every other part scales the hit
/// to `damage / 4 + min(1, damage)`.
pub fn damage_on_part(part: DragonPart, damage: f32) -> f32 {
    if part == DragonPart::Head {
        damage
    } else {
        damage * 0.25 + damage.min(1.0)
    }
}
/// The dragon smashes blocks every this many ticks.
pub const SMASH_INTERVAL: u64 = 3;
/// A fireball is spat at the nearest player in [`FIREBALL_RANGE`] every `FIREBALL_PERIOD` ticks.
pub const FIREBALL_PERIOD: u32 = 400;
pub const FIREBALL_RANGE: f64 = 64.0;
pub const FIREBALL_SPEED: f64 = 1.0;
/// Minimum ticks between two fireballs; stored in the persisted `bow_timer`.
pub const FIREBALL_COOLDOWN: u16 = 100;
/// The dragon smashes every block inside its body except these (`#minecraft:dragon_immune`).
pub const IMMUNE: &[&str] = &[
    "minecraft:barrier",
    "minecraft:bedrock",
    "minecraft:end_portal",
    "minecraft:end_portal_frame",
    "minecraft:end_gateway",
    "minecraft:command_block",
    "minecraft:repeating_command_block",
    "minecraft:chain_command_block",
    "minecraft:structure_block",
    "minecraft:jigsaw",
    "minecraft:moving_piston",
    "minecraft:obsidian",
    "minecraft:crying_obsidian",
    "minecraft:end_stone",
    "minecraft:iron_bars",
    "minecraft:respawn_anchor",
    "minecraft:reinforced_deepslate",
    "minecraft:test_block",
    "minecraft:test_instance_block",
];
/// Crystals within this many blocks heal the dragon (`EnderDragon.checkCrystals`).
pub const HEAL_RANGE: f64 = 32.0;
/// The dragon heals this much every [`HEAL_INTERVAL`] ticks while a crystal is in range.
pub const HEAL_INTERVAL: u32 = 10;
pub const HEAL_AMOUNT: f32 = 1.0;

fn valid_target(target: &Target, from: Vec3) -> bool {
    target.pos.x.is_finite()
        && target.pos.y.is_finite()
        && target.pos.z.is_finite()
        && target.pos.distance_to_sqr(from) <= TARGET_RANGE * TARGET_RANGE
}

fn select_target(players: &[Target], from: Vec3, current: Option<Uuid>) -> Option<Target> {
    if let Some(current) = current
        && let Some(target) = players
            .iter()
            .find(|target| target.uuid == current && valid_target(target, from))
    {
        return Some(*target);
    }
    players
        .iter()
        .filter(|target| valid_target(target, from))
        .min_by(|a, b| {
            a.pos
                .distance_to_sqr(from)
                .total_cmp(&b.pos.distance_to_sqr(from))
                .then_with(|| a.uuid.cmp(&b.uuid))
        })
        .copied()
}

/// An end crystal standing at `pos` (the block position above its base, centred).
pub fn new_crystal(pos: Vec3) -> EntityRecord {
    let props = ferrite_data::entity_props::by_name("minecraft:end_crystal")
        .expect("the vanilla data knows the end crystal");
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data: EntityData::new(
            allocate_id(),
            random_uuid(),
            props.id,
            f64::from(props.width),
            f64::from(props.height),
        ),
    }
}

/// The entity type id of the end crystal.
pub fn crystal_type() -> u16 {
    ferrite_data::entity_props::by_name("minecraft:end_crystal").map_or(u16::MAX, |p| p.id)
}

/// Whether `key` is a standing end crystal.
pub fn is_crystal(store: &EntityStore, key: EntityKey) -> bool {
    store
        .data(key)
        .is_some_and(|data| data.kind == crystal_type())
        && store
            .flags(key)
            .is_some_and(|flags| !flags.has(Flags::REMOVED))
}

/// Every end crystal an arrow is inside of: crystal, arrow and the crystal position.
pub fn arrows_in_crystals(store: &EntityStore) -> Vec<(EntityKey, EntityKey, Vec3)> {
    let arrow_type = ferrite_data::entity_props::by_name("minecraft:arrow").map(|p| p.id);
    let mut hits = Vec::new();
    for key in store.keys().to_vec() {
        if !is_crystal(store, key) {
            continue;
        }
        let (Some(bbox), Some(pos)) = (store.bbox(key), store.pos(key)) else {
            continue;
        };
        let arrow = store.query_vec(bbox).into_iter().find(|&other| {
            store.data(other).map(|data| data.kind) == arrow_type
                && store
                    .flags(other)
                    .is_some_and(|flags| !flags.has(Flags::REMOVED))
        });
        if let Some(arrow) = arrow {
            hits.push((key, arrow, pos));
        }
    }
    hits
}

/// Lets every living dragon of the store regain health from crystals within [`HEAL_RANGE`].
pub fn heal_from_crystals(store: &mut EntityStore) {
    for key in store.keys().to_vec() {
        let Some(state) = mob::state(store, key) else {
            continue;
        };
        if state.kind != mob::MobKind::EnderDragon
            || state.is_dead()
            || !state.wander_timer.is_multiple_of(HEAL_INTERVAL)
        {
            continue;
        }
        let Some(pos) = store.pos(key) else { continue };
        let area = AABB::new(
            pos.x - HEAL_RANGE,
            pos.y - HEAL_RANGE,
            pos.z - HEAL_RANGE,
            pos.x + HEAL_RANGE,
            pos.y + HEAL_RANGE,
            pos.z + HEAL_RANGE,
        );
        let near = store
            .query_vec(area)
            .into_iter()
            .any(|other| is_crystal(store, other));
        if near && let Some(state) = mob::state_mut(store, key) {
            state.health = (state.health + HEAL_AMOUNT).min(state.kind.max_health());
        }
    }
}

/// Advances one dragon by a tick. The age lives in the mob's `wander_timer`; hits on players
/// that get in its way are reported as melee events.
///
/// The landing branch uses vanilla's holding-pattern decision: after a fixed holding window the
/// dragon lands with probability `1 / (alive crystals + 3)`, descends to the podium, breathes
/// while perched, then climbs back to its circling path. The dive and fireball cadence remains
/// the simplified attack cycle represented by the existing constants.
pub fn fly(
    store: &mut EntityStore,
    key: EntityKey,
    players: &[Target],
    rng: &mut Rng,
    events: &mut Vec<MobEvent>,
) {
    let (age, mut phase, mut phase_ticks, mut attack_cooldown, mut bow_timer, saved_target) =
        match mob::state_mut(store, key) {
            Some(state) => {
                state.wander_timer = state.wander_timer.wrapping_add(1);
                state.attack_cooldown = state.attack_cooldown.saturating_sub(1);
                state.bow_timer = state.bow_timer.saturating_sub(1);
                (
                    state.wander_timer,
                    state.dragon_phase,
                    state.dragon_phase_ticks.saturating_add(1),
                    state.attack_cooldown,
                    state.bow_timer,
                    state.target,
                )
            }
            None => return,
        };
    let previous = store.pos(key).unwrap_or_default();
    let selected = select_target(players, previous, saved_target);
    let charge_target = selected
        .filter(|target| target.pos.distance_to_sqr(previous) <= CHARGE_RANGE * CHARGE_RANGE);
    let combat_phase = Phase::at(age, charge_target.is_some());
    let (circle, circle_yaw) = position_at(age);
    let alive_crystals = store
        .keys()
        .iter()
        .filter(|&&other| is_crystal(store, other))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    // The holding-pattern decision happens only at the end of a window, not every tick.
    if phase == DragonPhase::Holding && phase_ticks >= HOLDING_DECISION_TICKS {
        let bound = i32::try_from(alive_crystals.saturating_add(3)).unwrap_or(i32::MAX);
        if should_land(alive_crystals, rng.next_int(bound) as u32) {
            phase = DragonPhase::LandingApproach;
        }
        phase_ticks = 0;
    }
    // Every `CHARGE_PERIOD` ticks the dragon dives at the nearest player for a while, then
    // rejoins the circle; between those it never moves faster than `MAX_SPEED`.
    let goal = match phase {
        DragonPhase::LandingApproach | DragonPhase::Perched => PERCH_SPOT,
        DragonPhase::Takeoff => circle,
        DragonPhase::Holding => match combat_phase {
            Phase::Circling => circle,
            Phase::Telegraphing => previous,
            Phase::Charging => charge_target.map_or(circle, |target| target.pos),
        },
    };
    let step = goal - previous;
    let length = (step.x * step.x + step.y * step.y + step.z * step.z).sqrt();
    let pos = if length <= MAX_SPEED {
        goal
    } else {
        previous
            + Vec3::new(
                step.x / length * MAX_SPEED,
                step.y / length * MAX_SPEED,
                step.z / length * MAX_SPEED,
            )
    };
    let look = if phase == DragonPhase::Holding
        && matches!(combat_phase, Phase::Telegraphing | Phase::Charging)
    {
        charge_target.map_or(step, |target| target.pos - previous)
    } else {
        step
    };
    let yaw = if look.length_sqr() > 0.25 {
        (-look.x).atan2(look.z).to_degrees() as f32
    } else {
        circle_yaw
    };
    store.set_pos(key, pos);
    store.set_vel(key, pos - previous);
    match phase {
        DragonPhase::LandingApproach
            if pos.distance_to_sqr(PERCH_SPOT) <= PERCH_REACH * PERCH_REACH =>
        {
            phase = DragonPhase::Perched;
            phase_ticks = 0;
        }
        DragonPhase::Perched if phase_ticks >= PERCH_TICKS => {
            phase = DragonPhase::Takeoff;
            phase_ticks = 0;
        }
        DragonPhase::Takeoff if pos.distance_to_sqr(circle) <= TAKEOFF_REACH * TAKEOFF_REACH => {
            phase = DragonPhase::Holding;
            phase_ticks = 0;
        }
        _ => {}
    }
    // Strafing: a fireball at the nearest player now and then, from the head.
    if phase == DragonPhase::Holding
        && combat_phase == Phase::Circling
        && bow_timer == 0
        && age % FIREBALL_PERIOD == FIREBALL_PERIOD / 2
        && let Some(target) = selected
            .filter(|target| target.pos.distance_to_sqr(pos) <= FIREBALL_RANGE * FIREBALL_RANGE)
    {
        let from = pos + Vec3::new(0.0, 1.0, 0.0);
        let direction = (target.pos - from).normalize();
        if direction.length_sqr() > 0.0 {
            events.push(MobEvent::DragonFireball { from, direction });
            bow_timer = FIREBALL_COOLDOWN;
        }
    }
    // Perched on the portal it breathes a cloud at its feet.
    if phase == DragonPhase::Perched
        && bow_timer == 0
        && phase_ticks.is_multiple_of(BREATH_INTERVAL)
    {
        events.push(MobEvent::Breath {
            pos: Vec3::new(pos.x, PERCH_SPOT.y - 3.0, pos.z),
        });
        bow_timer = BREATH_INTERVAL as u16;
    }
    if let Some(data) = store.data_mut(key) {
        data.yaw = yaw;
        data.head_yaw = yaw;
        data.pitch = 0.0;
    }
    if attack_cooldown == 0
        && let Some(target) = players
            .iter()
            .find(|target| target.pos.distance_to_sqr(pos) <= CONTACT_RANGE * CONTACT_RANGE)
    {
        events.push(MobEvent::Melee {
            player: target.uuid,
            damage: CONTACT_DAMAGE,
            from: pos,
            attacker: key,
        });
        attack_cooldown = CONTACT_COOLDOWN;
    }
    if let Some(state) = mob::state_mut(store, key) {
        state.target = selected.map(|target| target.uuid);
        state.attack_cooldown = attack_cooldown;
        state.bow_timer = bow_timer;
        state.dragon_phase = phase;
        state.dragon_phase_ticks = phase_ticks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rng::Rng, store::EntityStore};

    #[test]
    fn the_dragon_circles_the_island_at_a_steady_radius() {
        for age in (0..2000).step_by(37) {
            let (pos, _) = position_at(age);
            let r = pos.x.hypot(pos.z);
            assert!((r - RADIUS).abs() < 1e-6, "age {age}: radius {r}");
            assert!((pos.y - HEIGHT).abs() <= BOB + 1e-6);
        }
        let (start, _) = position_at(0);
        assert_eq!((start.x, start.z), (CENTER[0] + RADIUS, CENTER[1]));
    }

    #[test]
    fn it_faces_where_it_flies() {
        let (a, yaw) = position_at(100);
        let (b, _) = position_at(101);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let flown = (-dx).atan2(dz).to_degrees() as f32;
        let mut diff = (flown - yaw).abs() % 360.0;
        if diff > 180.0 {
            diff = 360.0 - diff;
        }
        assert!(diff < 2.0, "yaw {yaw} vs heading {flown}");
    }

    #[test]
    fn crystals_heal_the_dragon_and_a_player_in_its_way_is_hit() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(1);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let mut events = Vec::new();
        fly(&mut store, key, &[], &mut rng, &mut events);
        mob::state_mut(&mut store, key).unwrap().health = 100.0;
        // No crystal: no healing, whatever the age.
        for _ in 0..30 {
            fly(&mut store, key, &[], &mut rng, &mut events);
            heal_from_crystals(&mut store);
        }
        assert_eq!(mob::state(&store, key).unwrap().health, 100.0);
        let here = store.pos(key).unwrap();
        store.insert(new_crystal(Vec3::new(here.x + 10.0, here.y, here.z)));
        for _ in 0..30 {
            fly(&mut store, key, &[], &mut rng, &mut events);
            heal_from_crystals(&mut store);
        }
        assert_eq!(mob::state(&store, key).unwrap().health, 103.0);
        // A player right under the dragon is hit once per cooldown.
        for _ in 0..21 {
            let target = Target {
                uuid: uuid::Uuid::from_u128(7),
                pos: store.pos(key).unwrap(),
            };
            fly(&mut store, key, &[target], &mut rng, &mut events);
        }
        let hits = events
            .iter()
            .filter(|e| matches!(e, MobEvent::Melee { damage, .. } if *damage == CONTACT_DAMAGE))
            .count();
        assert_eq!(hits, 2);
    }

    #[test]
    fn the_dragon_dives_at_a_player_and_comes_back_to_the_circle() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let player = Target {
            uuid: uuid::Uuid::from_u128(9),
            pos: Vec3::new(10.0, 70.0, 10.0),
        };
        let mut events = Vec::new();
        let mut closest = f64::MAX;
        for _ in 0..(CHARGE_PERIOD + 200) {
            fly(&mut store, key, &[player], &mut rng, &mut events);
            closest = closest.min(store.pos(key).unwrap().distance_to_sqr(player.pos));
        }
        assert!(
            closest.sqrt() < CONTACT_RANGE,
            "it got within {}",
            closest.sqrt()
        );
        assert!(!events.is_empty(), "the dive hits the player");
        // Without a player it holds the circle.
        let mut calm = EntityStore::new(4);
        let key = calm.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        for _ in 0..(CHARGE_PERIOD + 200) {
            fly(&mut calm, key, &[], &mut rng, &mut Vec::new());
        }
        let age = CHARGE_PERIOD + 200;
        assert!(calm.pos(key).unwrap().distance_to_sqr(position_at(age).0) < 1e-9);
    }

    #[test]
    fn an_arrow_inside_a_crystal_is_found() {
        let mut store = EntityStore::new(4);
        let crystal = store.insert(new_crystal(Vec3::new(5.5, 100.0, 5.5)));
        assert!(arrows_in_crystals(&store).is_empty());
        let mut rng = Rng::new(1);
        let arrow = store.insert(crate::arrow::new_arrow(
            Vec3::new(5.5, 100.5, 5.5),
            Vec3::ZERO,
            2.0,
            None,
            &mut rng,
        ));
        let hits = arrows_in_crystals(&store);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].0, hits[0].1), (crystal, arrow));
        store.flags_mut(arrow).unwrap().set(Flags::REMOVED, true);
        assert!(
            arrows_in_crystals(&store).is_empty(),
            "a spent arrow breaks nothing"
        );
    }

    #[test]
    fn a_spawned_dragon_has_vanilla_health_and_moves_each_tick() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let record = mob::new_mob(
            mob::MobKind::EnderDragon,
            Vec3::new(0.0, 80.0, 0.0),
            &mut rng,
        );
        let key = store.insert(record);
        assert_eq!(mob::state(&store, key).unwrap().health, 200.0);
        let before = store.pos(key).unwrap();
        let mut events = Vec::new();
        fly(&mut store, key, &[], &mut rng, &mut events);
        fly(&mut store, key, &[], &mut rng, &mut events);
        let after = store.pos(key).unwrap();
        assert_ne!(before, after);
        assert!(mob::is_mob(&store, key));
        // It can be killed like any mob.
        assert!(matches!(
            mob::hurt(&mut store, key, 500.0, None),
            Some(mob::Hurt::Died)
        ));
    }

    #[test]
    fn alive_crystal_count_controls_the_landing_roll() {
        assert!(should_land(0, 0));
        assert!(!should_land(0, 1));
        assert!(should_land(0, 3));
        assert!(should_land(10, 0));
        assert!(!should_land(10, 12));
        assert!(should_land(10, 13));
    }

    #[test]
    fn it_spits_fireballs_while_holding() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let player = Target {
            uuid: uuid::Uuid::from_u128(3),
            pos: position_at(FIREBALL_PERIOD / 2).0,
        };
        let mut events = Vec::new();
        for _ in 0..FIREBALL_PERIOD {
            fly(&mut store, key, &[player], &mut rng, &mut events);
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MobEvent::DragonFireball { .. }))
        );
    }

    #[test]
    fn a_player_outside_the_charge_range_does_not_pull_it_off_the_circle() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let player = Target {
            uuid: uuid::Uuid::from_u128(4),
            pos: Vec3::new(CHARGE_RANGE + 50.0, 90.0, 0.0),
        };
        let mut events = Vec::new();
        for _ in 0..(CHARGE_PERIOD + 50) {
            fly(&mut store, key, &[player], &mut rng, &mut events);
        }
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, MobEvent::Melee { .. })),
            "an out-of-range target is not attacked"
        );
        assert_eq!(
            mob::state(&store, key).unwrap().dragon_phase,
            DragonPhase::Holding
        );
    }

    #[test]
    fn only_the_head_takes_full_damage() {
        let parts = part_boxes(Vec3::new(0.0, 80.0, 0.0), 0.0);
        let head = parts
            .iter()
            .find(|(part, _)| *part == DragonPart::Head)
            .unwrap()
            .1;
        assert_eq!(head.center(), Vec3::new(0.0, 83.0, 5.0));
        let wings: Vec<_> = parts
            .iter()
            .filter(|(part, _)| *part == DragonPart::Wing)
            .collect();
        assert_eq!(wings.len(), 2);
        assert_eq!(wings[0].1.center().x, 4.0);
        assert_eq!(wings[1].1.center().x, -4.0);
        assert_eq!(damage_on_part(DragonPart::Head, 10.0), 10.0);
        assert_eq!(damage_on_part(DragonPart::Body, 10.0), 3.5);
        assert_eq!(damage_on_part(DragonPart::Wing, 1.0), 1.25);
    }

    #[test]
    fn breath_cloud_grows_and_hits_after_its_wait() {
        let mut cloud = BreathCloud::fireball(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(cloud.radius(), BREATH_RADIUS);
        for _ in 0..BREATH_WAIT_TICKS {
            assert_eq!(cloud.advance(), Some(0.0), "still waiting");
        }
        for _ in 0..(BREATH_HIT_INTERVAL - 1) {
            assert_eq!(cloud.advance(), Some(0.0), "between hits");
        }
        assert_eq!(cloud.advance(), Some(BREATH_DAMAGE));
        assert!(cloud.radius() > BREATH_RADIUS);
        for _ in 0..BREATH_TICKS {
            if cloud.advance().is_none() {
                break;
            }
        }
        assert_eq!(cloud.advance(), None, "the cloud expires");
        assert!((cloud.radius() - BREATH_MAX_RADIUS).abs() < 1e-9);
    }

    #[test]
    fn it_lands_perches_breathes_and_takes_off() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        mob::state_mut(&mut store, key).unwrap().dragon_phase = DragonPhase::LandingApproach;
        let mut events = Vec::new();
        let mut saw_perched = false;
        let mut saw_takeoff = false;
        for _ in 0..(PERCH_TICKS + 120) {
            fly(&mut store, key, &[], &mut rng, &mut events);
            match mob::state(&store, key).unwrap().dragon_phase {
                DragonPhase::Perched => saw_perched = true,
                DragonPhase::Takeoff => saw_takeoff = true,
                _ => {}
            }
        }
        assert!(saw_perched, "the approach ends in a perch");
        assert!(saw_takeoff, "the perch ends in takeoff");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MobEvent::Breath { .. })),
            "the perched dragon breathes"
        );
        assert_eq!(
            mob::state(&store, key).unwrap().dragon_phase,
            DragonPhase::Holding,
            "it returns to the holding pattern after takeoff"
        );
    }
}
