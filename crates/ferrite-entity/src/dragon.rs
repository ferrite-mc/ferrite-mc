//! The ender dragon's flight: a slow circle, a strafe at nearby players, a dive, a fireball,
//! and a landing cycle around the exit portal.
//!
//! The circle is the holding pattern of vanilla's phase machine. The landing decision and its
//! approach/perch/takeoff phases plus the strafe orbit are modelled explicitly; pathfinding is
//! still replaced by deterministic geometry.

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
    Strafe,
    LandingApproach,
    /// Perched and scanning for a player (vanilla `DragonSittingScanningPhase`). The name and
    /// variant predate the subphases, so older saves of a perched dragon load as scanning.
    Perched,
    /// Perched and roaring at a player before it flames (`DragonSittingAttackingPhase`).
    PerchAttacking,
    /// Perched and breathing flame clouds (`DragonSittingFlamingPhase`).
    PerchFlaming,
    Takeoff,
    /// Flying at a position fixed when the scan gave up (`DragonChargePlayerPhase`).
    ChargePlayer,
}

impl DragonPhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Holding => "holding",
            Self::Strafe => "strafe",
            Self::LandingApproach => "landing_approach",
            Self::Perched => "perched",
            Self::PerchAttacking => "perch_attacking",
            Self::PerchFlaming => "perch_flaming",
            Self::Takeoff => "takeoff",
            Self::ChargePlayer => "charge_player",
        }
    }

    /// Vanilla `EnderDragonPhase` protocol id (`0..=10`) for synched metadata index 16.
    pub const fn protocol_id(self) -> i32 {
        match self {
            Self::Holding => 0,
            Self::Strafe => 1,
            Self::LandingApproach => 2,
            Self::Takeoff => 4,
            Self::PerchFlaming => 5,
            Self::Perched => 6,
            Self::PerchAttacking => 7,
            Self::ChargePlayer => 8,
        }
    }

    /// `DragonPhaseInstance.isSitting`: perched phases use the fixed `-1` head height.
    pub fn is_sitting(self) -> bool {
        matches!(
            self,
            Self::Perched | Self::PerchAttacking | Self::PerchFlaming
        )
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "holding" => Some(Self::Holding),
            "strafe" => Some(Self::Strafe),
            "landing_approach" => Some(Self::LandingApproach),
            "perched" => Some(Self::Perched),
            "perch_attacking" => Some(Self::PerchAttacking),
            "perch_flaming" => Some(Self::PerchFlaming),
            "takeoff" => Some(Self::Takeoff),
            "charge_player" => Some(Self::ChargePlayer),
            _ => None,
        }
    }
}

/// Vanilla `EnderDragon.DATA_PHASE` (index 16, Int).
pub const PHASE_INDEX: u8 = 16;

/// Defines `EnderDragon.DATA_PHASE` on synched entity data and writes `phase`.
pub fn define_metadata(metadata: &mut crate::Metadata, phase: DragonPhase) {
    metadata.define(
        PHASE_INDEX,
        ferrite_protocol::packets::play::MetaValue::Int(0),
    );
    sync_metadata(metadata, phase);
}

/// Updates `EnderDragon.DATA_PHASE` from `phase`.
pub fn sync_metadata(metadata: &mut crate::Metadata, phase: DragonPhase) {
    metadata.set(
        PHASE_INDEX,
        ferrite_protocol::packets::play::MetaValue::Int(phase.protocol_id()),
    );
}

/// Ticks of circling before the dragon decides whether to land. Vanilla makes that decision
/// when a path ends; this window keeps the decision deterministic without modelling the path.
pub const HOLDING_DECISION_TICKS: u32 = 1200;
/// The dragon starts its approach after this many holding-window ticks when the roll succeeds,
/// with a chance of `1 / (alive crystals + 3)`.
pub fn should_land(alive_crystals: u32, roll: u32) -> bool {
    roll.is_multiple_of(alive_crystals.saturating_add(3))
}

/// The holding pattern's choice when its decision window ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingDecision {
    Land,
    Strafe,
    Hold,
}

/// Vanilla `DragonHoldingPatternPhase.findNewTarget`: landing wins with probability
/// `1 / (crystals + 3)`; otherwise the dragon strafes a nearby player or keeps circling.
pub fn holding_decision(alive_crystals: u32, roll: u32, has_target: bool) -> HoldingDecision {
    if should_land(alive_crystals, roll) {
        HoldingDecision::Land
    } else if has_target {
        HoldingDecision::Strafe
    } else {
        HoldingDecision::Hold
    }
}
/// The distance to the perch spot at which the approach becomes a perch.
pub const PERCH_REACH: f64 = 4.0;
pub const PERCH_SPOT: Vec3 = Vec3::new(0.0, 68.0, 0.0);
/// Perched subphase timings, from vanilla's sitting phases as remembered (not checked against
/// the 26.3 jar): a scanning dragon with no player near takes off after 100 ticks; with a player
/// within `SCAN_RANGE` it starts attacking once it has scanned for more than 25 ticks; the
/// attack roars for 40 ticks, then the dragon flames for 200 ticks, spreading four breath
/// clouds, and goes back to scanning.
pub const SCAN_IDLE_TICKS: u32 = 100;
pub const SCAN_ATTACK_DELAY: u32 = 25;
pub const SCAN_RANGE: f64 = 20.0;
/// Vanilla's scan box is 20 blocks horizontally and 10 blocks vertically.
pub const SCAN_RANGE_Y: f64 = 10.0;
pub const ATTACK_TICKS: u32 = 40;
/// One flaming phase lasts 200 ticks and creates a single cloud after a 10-tick warm-up.
/// Vanilla then returns to scanning unless four flaming phases finished, in which case it takes
/// off (`DragonSittingFlamingPhase` FLAME_DURATION, WARMUP_TIME and SITTING_FLAME_ATTACKS_COUNT).
pub const FLAME_TICKS: u32 = 200;
pub const FLAME_ATTACKS: u8 = 4;
pub const FLAME_WARMUP_TICKS: u32 = 10;
/// A scanning dragon that gives up charges the nearest player within this many blocks instead of
/// taking off (`DragonSittingScanningPhase.CHARGE_TARGETING`, range 150, jar-checked).
pub const SCAN_CHARGE_RANGE: f64 = 150.0;
/// `DragonChargePlayerPhase` starts its 10-tick recovery once it is within 10 blocks of the fixed
/// target (distance squared below 100) or the target is beyond 150 blocks (22500 squared).
pub const CHARGE_REACH: f64 = 10.0;
pub const CHARGE_FAR: f64 = 150.0;
pub const CHARGE_RECOVERY_TICKS: u32 = 10;
/// `DragonChargePlayerPhase.getFlySpeed()` (jar-checked, 3.0; every other phase uses 0.6). In
/// `EnderDragon.aiStep` it is not a speed: it clamps the vertical slope `dy / horizontal distance`
/// toward the phase target, so a charge may dive far more steeply than other phases.
pub const CHARGE_FLY_SPEED: f64 = 3.0;
/// A scanning dragon turns toward the player only while its heading is off by more than this
/// many degrees (`acos(dot) * 57.29 + 0.5` outside `0..=10`).
pub const SCAN_TURN_THRESHOLD: f32 = 10.0;
/// Vanilla `EnderDragon.aiStep` places the `head` (`1.0x1.0` `EnderDragonPart`) 6.5 blocks in front
/// of the body, and `EnderDragon.getHeadYOffset()` returns `-1.0` while sitting; a breath cloud
/// forms half the 5-block spray length further on (`DragonSittingFlamingPhase`:
/// `head.x/z + direction * 5 / 2`, `head.getY(0.5)`).
pub const HEAD_OFFSET: f64 = 6.5;
pub const HEAD_HEIGHT: f64 = 1.0;
pub const HEAD_SITTING_Y_OFFSET: f64 = -1.0;
pub const BREATH_AIM_OFFSET: f64 = 2.5;
/// The distance to the circling path at which takeoff ends.
pub const TAKEOFF_REACH: f64 = 4.0;
/// Vanilla `DragonStrafePlayerPhase`: the dragon flies around the selected player and fires a
/// fireball once it has a shot, then returns to the holding pattern.
pub const STRAFE_RADIUS: f64 = 32.0;
pub const STRAFE_HEIGHT: f64 = 8.0;
pub const STRAFE_ANGULAR_SPEED: f64 = 0.04;
/// Ticks the dragon charges its fireball while strafing (vanilla `FIREBALL_CHARGE_AMOUNT`).
pub const STRAFE_FIREBALL_TICKS: u32 = 5;
/// Vanilla leaves the strafe phase after a long chase; this bounds a target that never lines up.
pub const STRAFE_TIMEOUT_TICKS: u32 = 600;

/// One point of the orbit flown during [`DragonPhase::Strafe`]. Vanilla targets the player's
/// position; Ferrite uses a deterministic circle around it so the phase is testable.
pub fn strafe_point(anchor: Vec3, ticks: u32) -> Vec3 {
    let angle = f64::from(ticks) * STRAFE_ANGULAR_SPEED;
    Vec3::new(
        anchor.x + STRAFE_RADIUS * angle.cos(),
        anchor.y + STRAFE_HEIGHT + 2.0 * (f64::from(ticks) * 0.1).sin(),
        anchor.z + STRAFE_RADIUS * angle.sin(),
    )
}
/// The heading of a dragon with `yaw`, as a horizontal unit vector.
fn heading(yaw: f32) -> (f64, f64) {
    let angle = f64::from(yaw).to_radians();
    (-angle.sin(), angle.cos())
}

/// Position of a perched dragon's `head` sub-entity (`EnderDragon.aiStep` + `getHeadYOffset()`
/// when sitting: 6.5 blocks along the heading and 1 block below the body feet).
pub fn sitting_head_pos(pos: Vec3, yaw: f32) -> Vec3 {
    let (hx, hz) = heading(yaw);
    Vec3::new(
        pos.x + hx * HEAD_OFFSET,
        pos.y + HEAD_SITTING_Y_OFFSET,
        pos.z + hz * HEAD_OFFSET,
    )
}

/// Vanilla `DragonSittingScanningPhase` turn: while the scanned player is more than
/// [`SCAN_TURN_THRESHOLD`] degrees off the heading, the yaw rate decays by 0.8 and gains
/// `clamp(wrapDegrees(angle to player - yaw), -100, 100) * (0.7 / f3 / f4)`, where `f4` is the
/// horizontal head-to-player distance plus one and `f3` is `f4` capped at 40. Returns the new
/// `(yaw, yaw_rate)`. The yaw here is the heading yaw Ferrite stores, which vanilla's model yaw
/// differs from by 180 degrees; the turn is the same.
pub fn scan_turn(yaw: f32, rate: f32, pos: Vec3, player: Vec3) -> (f32, f32) {
    let head = sitting_head_pos(pos, yaw);
    let to_player = Vec3::new(player.x - pos.x, 0.0, player.z - pos.z).normalize();
    let (hx, hz) = heading(yaw);
    let dot = (to_player.x * hx + to_player.z * hz) as f32;
    let angle = f64::from(dot).acos() as f32 * 57.295_776 + 0.5;
    if !(angle < 0.0 || angle > SCAN_TURN_THRESHOLD) {
        return (yaw, rate);
    }
    let d0 = player.x - head.x;
    let d1 = player.z - head.z;
    let wanted = (-d0).atan2(d1).to_degrees();
    let delta = wrap_degrees(wanted - f64::from(yaw)).clamp(-100.0, 100.0);
    let f4 = ((d0 * d0 + d1 * d1).sqrt() + 1.0) as f32;
    let f3 = f4.min(40.0);
    let rate = rate * 0.8 + delta as f32 * (0.7 / f3 / f4);
    (yaw + rate, rate)
}

/// Limits the vertical part of `step` to `fly_speed` times its horizontal length, as
/// `EnderDragon.aiStep` clamps `dy / horizontal` to `+-getFlySpeed()`. A step with no horizontal
/// part is left alone (vanilla skips the clamp when the horizontal distance is zero).
pub fn clamp_slope(step: Vec3, fly_speed: f64) -> Vec3 {
    let horizontal = (step.x * step.x + step.z * step.z).sqrt();
    if horizontal <= 0.0 {
        return step;
    }
    let limit = fly_speed * horizontal;
    Vec3::new(step.x, step.y.clamp(-limit, limit), step.z)
}

/// Vanilla `EnderDragon.aiStep` smooth flight steering: turns yaw gradually using decaying
/// `yaw_rate` (`rate * 0.8 + clamp(delta, -50, 50) * 0.1`) so the dragon banks smoothly toward
/// flight targets instead of snapping instantaneously.
pub fn fly_turn_step(yaw: f32, yaw_rate: f32, pos: Vec3, target: Vec3) -> (f32, f32) {
    let dx = target.x - pos.x;
    let dz = target.z - pos.z;
    if dx * dx + dz * dz < 1.0e-6 {
        let rate = yaw_rate * 0.8;
        return (yaw + rate, rate);
    }
    let wanted = (-dx).atan2(dz).to_degrees();
    let delta = wrap_degrees(wanted - f64::from(yaw)).clamp(-50.0, 50.0);
    let rate = yaw_rate * 0.8 + delta as f32 * 0.1;
    (yaw + rate, rate)
}

fn wrap_degrees(angle: f64) -> f64 {
    let wrapped = angle.rem_euclid(360.0);
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else {
        wrapped
    }
}

/// Where the perched breath cloud forms horizontally, and the height the search for ground
/// starts at: the head position pushed half the spray length forward
/// (`DragonSittingFlamingPhase.doServerTick`), at the head's mid height (`head.getY(0.5)`).
pub fn breath_aim(pos: Vec3, yaw: f32) -> Vec3 {
    let (hx, hz) = heading(yaw);
    let head = sitting_head_pos(pos, yaw);
    Vec3::new(
        head.x + hx * BREATH_AIM_OFFSET,
        head.y + HEAD_HEIGHT * 0.5,
        head.z + hz * BREATH_AIM_OFFSET,
    )
}

/// The cloud height under `aim`: vanilla walks down from the head's height while the block is
/// air; reaching the bottom of the world falls back to the head height; the cloud sits on top of
/// the block it found.
pub fn breath_floor(head_y: f64, bottom: i32, is_empty: impl Fn(i32) -> bool) -> f64 {
    let mut y = head_y;
    while is_empty(y.floor() as i32) {
        y -= 1.0;
        if y < f64::from(bottom) {
            y = head_y;
            break;
        }
    }
    y.floor() + 1.0
}

/// A dragon fireball's cloud starts at radius 3, waits 20 ticks, then grows to radius 5 over its
/// 30-second life (vanilla `DragonFireball`). The perched breath has its own constant
/// radius-5, 200-tick shape (`PERCHED_BREATH_*`). Both apply `MobEffects.INSTANT_DAMAGE` as a
/// vanilla `AreaEffectCloud` (3 magic damage every 20 ticks).
pub const BREATH_TICKS: u32 = crate::potion::PotionCloud::DRAGON_FIREBALL_DURATION;
pub const BREATH_RADIUS: f64 = crate::potion::PotionCloud::DRAGON_FIREBALL_RADIUS as f64;
pub const BREATH_MAX_RADIUS: f64 = crate::potion::PotionCloud::DRAGON_FIREBALL_MAX_RADIUS as f64;
pub const BREATH_WAIT_TICKS: u32 = crate::potion::PotionCloud::DEFAULT_WAIT;
pub const BREATH_HIT_INTERVAL: u32 = crate::potion::PotionCloud::REAPPLICATION_DELAY;

/// The perched dragon's cloud (`DragonSittingFlamingPhase`, read with `javap` from the 26.3
/// jar): `setRadius(5.0)` and `setDuration(200)` with no radius growth, created once when the
/// flame counter reaches 10; `AreaEffectCloud` keeps its default 20-tick wait before it hurts.
pub const PERCHED_BREATH_TICKS: u32 = crate::potion::PotionCloud::DRAGON_BREATH_DURATION;
pub const PERCHED_BREATH_RADIUS: f64 = crate::potion::PotionCloud::DRAGON_BREATH_RADIUS as f64;
pub const PERCHED_BREATH_WAIT_TICKS: u32 = crate::potion::PotionCloud::DEFAULT_WAIT;

/// Creates the `AreaEffectCloud` for a perched dragon breath at `center`, owned by `owner`.
pub fn perched_breath_cloud(center: Vec3, owner: Option<Uuid>) -> crate::potion::PotionCloud {
    crate::potion::PotionCloud::dragon_breath(center).with_owner(owner)
}

/// Creates the `AreaEffectCloud` for a dragon fireball at `center`, owned by `owner`.
pub fn fireball_breath_cloud(center: Vec3, owner: Option<Uuid>) -> crate::potion::PotionCloud {
    crate::potion::PotionCloud::dragon_fireball(center).with_owner(owner)
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

/// Ticks of flight history the vanilla part placement reads: the head/neck use samples
/// `0..=10` and the three tails `12..=16` (`DragonFlightHistory.get`).
pub const FLIGHT_HISTORY: usize = 17;

/// The tail of `EnderDragon.flightHistory` used by [`part_boxes_with_flight`]: one height and
/// yaw per tick, index 0 being the newest sample. Vanilla keeps 64; 17 cover every offset the
/// dragon's parts use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragonFlight {
    y: [f32; FLIGHT_HISTORY],
    yaw: [f32; FLIGHT_HISTORY],
    filled: u8,
}

impl Default for DragonFlight {
    fn default() -> Self {
        Self {
            y: [0.0; FLIGHT_HISTORY],
            yaw: [0.0; FLIGHT_HISTORY],
            filled: 0,
        }
    }
}

impl DragonFlight {
    /// Records this tick's body height and yaw (`DragonFlightHistory.record`).
    pub fn record(&mut self, y: f32, yaw: f32) {
        self.y.rotate_right(1);
        self.yaw.rotate_right(1);
        self.y[0] = y;
        self.yaw[0] = yaw;
        self.filled = self.filled.saturating_add(1);
    }

    /// Height `back` ticks ago; before the history fills, the oldest sample repeats.
    pub fn y(&self, back: usize) -> f32 {
        Self::sample(&self.y, self.filled, back)
    }

    /// Yaw `back` ticks ago.
    pub fn yaw(&self, back: usize) -> f32 {
        Self::sample(&self.yaw, self.filled, back)
    }

    fn sample(values: &[f32; FLIGHT_HISTORY], filled: u8, back: usize) -> f32 {
        let filled = usize::from(filled).max(1);
        values[back.min(filled - 1).min(FLIGHT_HISTORY - 1)]
    }
}

/// Vanilla hitboxes: head 1x1, neck 3x3, body 3 high x 5 wide, three 2x2 tail parts and two
/// 2 high x 4 wide wings. [`part_boxes_with_flight`] places every part from the 26.3
/// `EnderDragon.aiStep` `tickPart` arguments, including the flight pitch and the tail yaw lag;
/// [`part_boxes`] is the same dragon with no recorded flight.
pub fn part_boxes(pos: Vec3, yaw: f32) -> [(DragonPart, AABB); DRAGON_PART_COUNT] {
    part_boxes_with_flight(pos, yaw, &DragonFlight::default(), false)
}

/// [`part_boxes`] for a dragon that has been flying. `sitting` mirrors
/// `DragonPhaseInstance.isSitting()`: `getHeadYOffset()` then returns `-1` instead of the
/// five-tick height change.
///
/// Ferrite stores the flight heading yaw, which is 180 degrees from the vanilla model yaw used
/// by `aiStep`; the head/neck and tail horizontal axes below are therefore flipped so the parts
/// stay on the side of the dragon the other Ferrite code (breath, scanning) treats as its front.
/// The vertical terms are exactly vanilla's.
pub fn part_boxes_with_flight(
    pos: Vec3,
    yaw: f32,
    flight: &DragonFlight,
    sitting: bool,
) -> [(DragonPart, AABB); DRAGON_PART_COUNT] {
    let angle = f64::from(yaw).to_radians();
    let (sin, cos) = angle.sin_cos();
    // `EnderDragon.aiStep`: f4 = (history.get(5).y - history.get(10).y) * 10 * 0.017453292 is
    // the flight pitch, f5 = cos(f4) scales horizontal offsets and f6 = sin(f4) is the rise.
    let pitch = f64::from((flight.y(5) - flight.y(10)) * 10.0 * 0.017_453_292_f32);
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    let head_y = if sitting {
        -1.0
    } else {
        f64::from(flight.y(5) - flight.y(0))
    };
    // `tickPart` takes the part's feet; the box grows upwards from there.
    let part = |dx: f64, dy: f64, dz: f64, width: f64, tall: f64| {
        let (x, y, z) = (pos.x + dx, pos.y + dy, pos.z + dz);
        AABB::new(
            x - width * 0.5,
            y,
            z - width * 0.5,
            x + width * 0.5,
            y + tall,
            z + width * 0.5,
        )
    };
    let mut boxes = [
        (
            DragonPart::Head,
            part(
                -sin * 6.5 * cos_pitch,
                head_y + 6.5 * sin_pitch,
                cos * 6.5 * cos_pitch,
                1.0,
                1.0,
            ),
        ),
        (
            DragonPart::Neck,
            part(
                -sin * 5.5 * cos_pitch,
                head_y + 5.5 * sin_pitch,
                cos * 5.5 * cos_pitch,
                3.0,
                3.0,
            ),
        ),
        // The body is half a block behind the pivot in Ferrite's heading.
        (DragonPart::Body, part(sin * 0.5, 0.0, -cos * 0.5, 5.0, 3.0)),
        (DragonPart::Tail, AABB::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
        (DragonPart::Tail, AABB::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
        (DragonPart::Tail, AABB::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
        (DragonPart::Wing, part(cos * 4.5, 2.0, sin * 4.5, 4.0, 2.0)),
        (
            DragonPart::Wing,
            part(cos * -4.5, 2.0, sin * -4.5, 4.0, 2.0),
        ),
    ];
    for i in 0..3 {
        let back = 12 + 2 * i;
        let tail_yaw =
            angle + wrap_degrees(f64::from(flight.yaw(back) - flight.yaw(5))).to_radians();
        let (tail_sin, tail_cos) = tail_yaw.sin_cos();
        let spread = (i + 1) as f64 * 2.0;
        let dx = (tail_sin * spread + sin * 1.5) * cos_pitch;
        let dz = -(tail_cos * spread + cos * 1.5) * cos_pitch;
        let dy = f64::from(flight.y(back) - flight.y(5)) - (spread + 1.5) * sin_pitch + 1.5;
        boxes[3 + i].1 = part(dx, dy, dz, 2.0, 2.0);
    }
    boxes
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

fn valid_position(pos: Vec3) -> bool {
    pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite()
}

fn valid_target(target: &Target, from: Vec3) -> bool {
    valid_position(target.pos) && target.pos.distance_to_sqr(from) <= TARGET_RANGE * TARGET_RANGE
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
    let mut data = EntityData::new(
        allocate_id(),
        random_uuid(),
        props.id,
        f64::from(props.width),
        f64::from(props.height),
    );
    // Verified from the 26.3 `EndCrystal.defineSynchedData`: an empty optional beam target and
    // the bottom plate shown by default.
    data.metadata.define(
        crate::metadata::end_crystal::BEAM_TARGET,
        ferrite_protocol::packets::play::MetaValue::OptionalBlockPos(None),
    );
    data.metadata.define(
        crate::metadata::end_crystal::SHOW_BOTTOM,
        ferrite_protocol::packets::play::MetaValue::Boolean(true),
    );
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
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
    let (
        age,
        mut phase,
        mut phase_ticks,
        mut attack_cooldown,
        mut bow_timer,
        saved_target,
        mut perch_flames,
    ) = match mob::state_mut(store, key) {
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
                state.perch_flames,
            )
        }
        None => return,
    };
    let previous = store.pos(key).unwrap_or_default();
    let (mut charge_target_pos, mut charge_recovery, mut yaw_rate) =
        mob::state(store, key).map_or((None, 0, 0.0), |state| {
            (
                state.dragon_charge_target,
                state.dragon_charge_recovery,
                state.dragon_yaw_rate,
            )
        });
    let stored_yaw = store.data(key).map_or(0.0, |data| data.yaw);
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
        match holding_decision(
            alive_crystals,
            rng.next_int(bound) as u32,
            selected.is_some(),
        ) {
            HoldingDecision::Land => {
                phase = DragonPhase::LandingApproach;
                perch_flames = 0;
            }
            HoldingDecision::Strafe => phase = DragonPhase::Strafe,
            HoldingDecision::Hold => {}
        }
        phase_ticks = 0;
    }
    // A strafe locks onto the player it started with; losing that player ends the phase.
    let strafe_target = if phase == DragonPhase::Strafe {
        let wanted = saved_target.or_else(|| selected.map(|target| target.uuid));
        players
            .iter()
            .copied()
            .find(|target| Some(target.uuid) == wanted && valid_target(target, previous))
    } else {
        None
    };
    // Every `CHARGE_PERIOD` ticks the dragon dives at the nearest player for a while, then
    // rejoins the circle; between those it never moves faster than `MAX_SPEED`.
    let goal = match phase {
        DragonPhase::Strafe => {
            strafe_target.map_or(previous, |target| strafe_point(target.pos, phase_ticks))
        }
        DragonPhase::LandingApproach
        | DragonPhase::Perched
        | DragonPhase::PerchAttacking
        | DragonPhase::PerchFlaming => PERCH_SPOT,
        DragonPhase::Takeoff => circle,
        DragonPhase::ChargePlayer => charge_target_pos.unwrap_or(previous),
        DragonPhase::Holding => match combat_phase {
            Phase::Circling => circle,
            Phase::Telegraphing => previous,
            Phase::Charging => charge_target.map_or(circle, |target| target.pos),
        },
    };
    let mut step = goal - previous;
    if phase == DragonPhase::ChargePlayer {
        step = clamp_slope(step, CHARGE_FLY_SPEED);
    }
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
    let look = if phase == DragonPhase::Strafe {
        strafe_target.map_or(step, |target| target.pos - previous)
    } else if phase == DragonPhase::Holding
        && matches!(combat_phase, Phase::Telegraphing | Phase::Charging)
    {
        charge_target.map_or(step, |target| target.pos - previous)
    } else {
        step
    };
    let mut yaw = if matches!(
        phase,
        DragonPhase::Perched | DragonPhase::PerchAttacking | DragonPhase::PerchFlaming
    ) {
        // A perched dragon only turns through the scan, never back to the circle's heading.
        stored_yaw
    } else if look.length_sqr() > 0.25 {
        (-look.x).atan2(look.z).to_degrees() as f32
    } else {
        circle_yaw
    };
    let flags = store.flags(key).unwrap_or_default();
    let collided = flags.has(Flags::HORIZONTAL_COLLISION) || flags.has(Flags::VERTICAL_COLLISION);
    store.set_pos(key, pos);
    store.set_vel(key, pos - previous);
    if flags.has(Flags::NO_PHYSICS)
        && let Some(entity_flags) = store.flags_mut(key)
    {
        // Vanilla `Entity.move`: a `noPhysics` entity clears collision flags when moving.
        entity_flags.set(Flags::HORIZONTAL_COLLISION, false);
        entity_flags.set(Flags::VERTICAL_COLLISION, false);
    }
    match phase {
        DragonPhase::Strafe if strafe_target.is_none() || phase_ticks >= STRAFE_TIMEOUT_TICKS => {
            phase = DragonPhase::Holding;
            phase_ticks = 0;
        }
        DragonPhase::LandingApproach
            if pos.distance_to_sqr(PERCH_SPOT) <= PERCH_REACH * PERCH_REACH =>
        {
            phase = DragonPhase::Perched;
            phase_ticks = 0;
        }
        // Scanning: a player in range for more than `SCAN_ATTACK_DELAY` ticks starts the attack
        // and the dragon turns toward that player; with nobody near, the dragon gives up after
        // `SCAN_IDLE_TICKS`, charging a player within `SCAN_CHARGE_RANGE` if there is one.
        DragonPhase::Perched => {
            let near = players
                .iter()
                .filter(|target| {
                    let dx = target.pos.x - pos.x;
                    let dz = target.pos.z - pos.z;
                    dx * dx + dz * dz <= SCAN_RANGE * SCAN_RANGE
                        && (target.pos.y - pos.y).abs() <= SCAN_RANGE_Y
                })
                .min_by(|a, b| {
                    a.pos
                        .distance_to_sqr(pos)
                        .total_cmp(&b.pos.distance_to_sqr(pos))
                        .then_with(|| a.uuid.cmp(&b.uuid))
                })
                .copied();
            if let Some(target) = near {
                (yaw, yaw_rate) = scan_turn(yaw, yaw_rate, pos, target.pos);
            }
            if near.is_some() && phase_ticks > SCAN_ATTACK_DELAY {
                phase = DragonPhase::PerchAttacking;
                phase_ticks = 0;
            } else if near.is_none() && phase_ticks >= SCAN_IDLE_TICKS {
                let victim = players
                    .iter()
                    .filter(|target| {
                        valid_position(target.pos)
                            && target.pos.distance_to_sqr(pos)
                                <= SCAN_CHARGE_RANGE * SCAN_CHARGE_RANGE
                    })
                    .min_by(|a, b| {
                        a.pos
                            .distance_to_sqr(pos)
                            .total_cmp(&b.pos.distance_to_sqr(pos))
                            .then_with(|| a.uuid.cmp(&b.uuid))
                    });
                if let Some(victim) = victim {
                    phase = DragonPhase::ChargePlayer;
                    charge_target_pos = Some(victim.pos);
                    charge_recovery = 0;
                } else {
                    phase = DragonPhase::Takeoff;
                }
                phase_ticks = 0;
            }
        }
        // Charging: fly at the fixed position. Within 10 blocks, when the target is more than 150
        // away, or on `horizontalCollision` / `verticalCollision` (`DragonChargePlayerPhase`),
        // `timeSinceCharge` starts; it counts up each tick and the phase ends on the tick that
        // finds it already at 10 (`timeSinceCharge++ >= 10`). Vanilla checks collisions in
        // `doServerTick` before `Entity.move` clears them for `noPhysics` dragons. A charge with
        // no target (e.g. an older or vanilla save without `DragonChargeTarget`) aborts at once.
        DragonPhase::ChargePlayer => match charge_target_pos {
            None => {
                phase = DragonPhase::Holding;
                phase_ticks = 0;
                charge_recovery = 0;
            }
            Some(target) => {
                let distance_sqr = target.distance_to_sqr(pos);
                if charge_recovery > 0 {
                    let before = charge_recovery;
                    charge_recovery += 1;
                    if before >= CHARGE_RECOVERY_TICKS {
                        phase = DragonPhase::Holding;
                        phase_ticks = 0;
                        charge_target_pos = None;
                        charge_recovery = 0;
                    }
                } else if distance_sqr < CHARGE_REACH * CHARGE_REACH
                    || distance_sqr > CHARGE_FAR * CHARGE_FAR
                    || collided
                {
                    charge_recovery = 1;
                }
            }
        },
        DragonPhase::PerchAttacking if phase_ticks >= ATTACK_TICKS => {
            phase = DragonPhase::PerchFlaming;
            phase_ticks = 0;
            // `DragonSittingFlamingPhase.begin` counts the flaming phase.
            perch_flames = perch_flames.saturating_add(1);
        }
        DragonPhase::PerchFlaming => {
            if phase_ticks == FLAME_WARMUP_TICKS {
                events.push(MobEvent::Breath {
                    pos: breath_aim(pos, yaw),
                    owner: store.data(key).map(|data| data.uuid),
                });
            }
            if phase_ticks >= FLAME_TICKS {
                // Four flaming phases end the perch; fewer go back to scanning.
                phase = if perch_flames >= FLAME_ATTACKS {
                    DragonPhase::Takeoff
                } else {
                    DragonPhase::Perched
                };
                phase_ticks = 0;
            }
        }
        DragonPhase::Takeoff if pos.distance_to_sqr(circle) <= TAKEOFF_REACH * TAKEOFF_REACH => {
            phase = DragonPhase::Holding;
            phase_ticks = 0;
        }
        _ => {}
    }
    // A finished strafe shot returns the dragon to the holding pattern, as vanilla
    // `DragonStrafePlayerPhase` does after launching its fireball.
    if phase == DragonPhase::Strafe
        && bow_timer == 0
        && phase_ticks >= STRAFE_FIREBALL_TICKS
        && let Some(target) = strafe_target
            .filter(|target| target.pos.distance_to_sqr(pos) <= FIREBALL_RANGE * FIREBALL_RANGE)
    {
        let from = pos + Vec3::new(0.0, 1.0, 0.0);
        let direction = (target.pos - from).normalize();
        if direction.length_sqr() > 0.0 {
            events.push(MobEvent::DragonFireball { from, direction });
            bow_timer = FIREBALL_COOLDOWN;
            phase = DragonPhase::Holding;
            phase_ticks = 0;
        }
    }
    // A fireball at the nearest player now and then, from the head, while holding.
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
        state.perch_flames = perch_flames;
        state.dragon_charge_target = charge_target_pos;
        state.dragon_charge_recovery = charge_recovery;
        state.dragon_yaw_rate = yaw_rate;
        state.dragon_flight.record(pos.y as f32, yaw);
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
    fn a_crystal_defines_its_beam_and_bottom_metadata() {
        let crystal = new_crystal(Vec3::new(0.5, 64.0, 0.5));
        assert_eq!(
            crystal
                .data
                .metadata
                .get(crate::metadata::end_crystal::BEAM_TARGET),
            Some(&ferrite_protocol::packets::play::MetaValue::OptionalBlockPos(None))
        );
        assert_eq!(
            crystal
                .data
                .metadata
                .get(crate::metadata::end_crystal::SHOW_BOTTOM),
            Some(&ferrite_protocol::packets::play::MetaValue::Boolean(true))
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
        assert_eq!(head.center(), Vec3::new(0.0, 80.5, 6.5));
        let body = parts
            .iter()
            .find(|(part, _)| *part == DragonPart::Body)
            .unwrap()
            .1;
        assert_eq!(body.center().y, 81.5);
        let wings: Vec<_> = parts
            .iter()
            .filter(|(part, _)| *part == DragonPart::Wing)
            .collect();
        assert_eq!(wings.len(), 2);
        assert_eq!(wings[0].1.center().x, 4.5);
        assert_eq!(wings[1].1.center().x, -4.5);
        assert_eq!(wings[0].1.center().y, 83.0);
        assert_eq!(wings[1].1.center().y, 83.0);
        assert_eq!(damage_on_part(DragonPart::Head, 10.0), 10.0);
        assert_eq!(damage_on_part(DragonPart::Body, 10.0), 3.5);
        assert_eq!(damage_on_part(DragonPart::Wing, 1.0), 1.25);

        let (yaw1, rate1) = fly_turn_step(0.0, 0.0, Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        assert!(rate1.abs() > 0.0 && yaw1.abs() < 15.0);
        let (yaw2, rate2) = fly_turn_step(yaw1, rate1, Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        assert!(rate2.abs() > rate1.abs() || yaw2.abs() > yaw1.abs());
    }

    #[test]
    fn breath_cloud_grows_and_hits_after_its_wait() {
        let mut cloud = fireball_breath_cloud(Vec3::new(1.0, 2.0, 3.0), None);
        let player = Uuid::from_u128(101);
        assert_eq!(f64::from(cloud.radius()), BREATH_RADIUS);
        for _ in 1..BREATH_WAIT_TICKS {
            assert_eq!(
                cloud.tick([(player, [1.0, 2.0, 3.0])]),
                Some(Vec::new()),
                "still waiting"
            );
        }
        let applied = cloud.tick([(player, [1.0, 2.0, 3.0])]).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].1.kind(), crate::effect::Effect::InstantDamage);
        assert!(f64::from(cloud.radius()) > BREATH_RADIUS);
        for _ in 0..BREATH_TICKS {
            if cloud.tick([]).is_none() {
                break;
            }
        }
        assert_eq!(cloud.tick([]), None, "the cloud expires");
        assert!((f64::from(cloud.radius()) - BREATH_MAX_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn every_dragon_phase_round_trips_through_its_saved_name() {
        for phase in [
            DragonPhase::Holding,
            DragonPhase::Strafe,
            DragonPhase::LandingApproach,
            DragonPhase::Perched,
            DragonPhase::PerchAttacking,
            DragonPhase::PerchFlaming,
            DragonPhase::Takeoff,
        ] {
            assert_eq!(DragonPhase::from_name(phase.name()), Some(phase));
        }
        // A dragon saved while perched, before the subphases existed, loads as scanning.
        assert_eq!(
            DragonPhase::from_name("perched"),
            Some(DragonPhase::Perched)
        );
        assert_eq!(DragonPhase::from_name("unknown"), None);
        assert_eq!(DragonPhase::default(), DragonPhase::Holding);
    }

    #[test]
    fn the_holding_pattern_lands_strafes_or_keeps_circling() {
        assert_eq!(holding_decision(0, 0, false), HoldingDecision::Land);
        assert_eq!(holding_decision(0, 1, true), HoldingDecision::Strafe);
        assert_eq!(holding_decision(0, 1, false), HoldingDecision::Hold);
        assert_eq!(holding_decision(10, 12, true), HoldingDecision::Strafe);
        assert_eq!(holding_decision(10, 13, true), HoldingDecision::Land);
    }

    #[test]
    fn the_strafe_orbit_circles_the_selected_player() {
        let anchor = Vec3::new(10.0, 70.0, -5.0);
        for ticks in (0..400).step_by(17) {
            let point = strafe_point(anchor, ticks);
            let radius = (point.x - anchor.x).hypot(point.z - anchor.z);
            assert!(
                (radius - STRAFE_RADIUS).abs() < 1e-6,
                "ticks {ticks}: radius {radius}"
            );
            assert!(point.y >= anchor.y + STRAFE_HEIGHT - 2.0);
            assert!(point.y <= anchor.y + STRAFE_HEIGHT + 2.0);
        }
    }

    #[test]
    fn landing_approach_perches_within_reach() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let key = store.insert(mob::new_mob(
            mob::MobKind::EnderDragon,
            PERCH_SPOT + Vec3::new(1.0, 0.0, 0.0),
            &mut rng,
        ));
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.dragon_phase = DragonPhase::LandingApproach;
            state.dragon_phase_ticks = 0;
        }
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Perched);
        assert_eq!(state.dragon_phase_ticks, 0);
    }

    fn perched(phase: DragonPhase) -> (EntityStore, EntityKey, Rng) {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let key = store.insert(mob::new_mob(
            mob::MobKind::EnderDragon,
            PERCH_SPOT,
            &mut rng,
        ));
        let state = mob::state_mut(&mut store, key).unwrap();
        state.dragon_phase = phase;
        state.dragon_phase_ticks = 0;
        (store, key, rng)
    }

    fn near_player() -> [Target; 1] {
        [Target {
            uuid: uuid::Uuid::from_u128(31),
            pos: PERCH_SPOT + Vec3::new(10.0, 0.0, 0.0),
        }]
    }

    /// Runs `n` ticks and returns the tick numbers (1-based) on which the dragon breathed.
    fn run(
        store: &mut EntityStore,
        key: EntityKey,
        rng: &mut Rng,
        players: &[Target],
        n: u32,
    ) -> Vec<u32> {
        let mut breaths = Vec::new();
        for tick in 1..=n {
            let mut events = Vec::new();
            fly(store, key, players, rng, &mut events);
            if events
                .iter()
                .any(|event| matches!(event, MobEvent::Breath { .. }))
            {
                breaths.push(tick);
            }
        }
        breaths
    }

    fn phase_of(store: &EntityStore, key: EntityKey) -> DragonPhase {
        mob::state(store, key).unwrap().dragon_phase
    }

    #[test]
    fn a_perched_breath_cloud_lives_200_ticks_at_a_constant_radius_5() {
        let owner = Uuid::from_u128(55);
        let mut cloud = perched_breath_cloud(Vec3::new(1.0, 2.0, 3.0), Some(owner));
        assert_eq!(cloud.lifetime(), PERCHED_BREATH_TICKS);
        assert_eq!(cloud.wait_ticks(), PERCHED_BREATH_WAIT_TICKS);
        assert_eq!(cloud.owner(), Some(owner));
        assert!((cloud.potion_duration_scale() - 0.25).abs() < 1e-6);
        assert_eq!(
            cloud.effects,
            vec![crate::effect::MobEffectInstance::new(
                crate::effect::Effect::InstantDamage.id(),
                0,
                0
            )]
        );
        let victim = Uuid::from_u128(77);
        for _ in 1..PERCHED_BREATH_TICKS {
            assert_eq!(cloud.radius(), 5.0, "no growth at age {}", cloud.age());
            assert!(cloud.tick([(victim, [1.0, 2.0, 3.0])]).is_some());
        }
        assert_eq!(cloud.radius(), 5.0);
        assert_eq!(
            cloud.tick([(victim, [1.0, 2.0, 3.0])]),
            None,
            "gone after 200 ticks, not 600"
        );
    }

    #[test]
    fn a_perched_breath_cloud_waits_then_applies_instant_damage_every_twenty_ticks() {
        let mut cloud = perched_breath_cloud(Vec3::ZERO, None);
        let victim = Uuid::from_u128(88);
        let mut hit_ticks = Vec::new();
        for tick in 1..=PERCHED_BREATH_TICKS {
            if let Some(applied) = cloud.tick([(victim, [0.0, 0.0, 0.0])])
                && !applied.is_empty()
            {
                assert_eq!(applied.len(), 1);
                assert_eq!(applied[0].1.kind(), crate::effect::Effect::InstantDamage);
                hit_ticks.push(tick);
            }
        }
        assert_eq!(
            hit_ticks,
            vec![20, 40, 60, 80, 100, 120, 140, 160, 180],
            "repeated 20-tick applications after the 20-tick wait"
        );
        assert_eq!(cloud.radius(), 5.0);
    }

    #[test]
    fn a_flaming_phase_makes_one_cloud_at_the_head_aim_and_four_phases_end_the_perch() {
        assert_eq!(
            (FLAME_WARMUP_TICKS, FLAME_TICKS, FLAME_ATTACKS),
            (10, 200, 4)
        );
        // Third flaming phase of four: one cloud at tick 10, then back to scanning.
        let (mut store, key, mut rng) = perched(DragonPhase::PerchFlaming);
        mob::state_mut(&mut store, key).unwrap().perch_flames = 3;
        let dragon_uuid = store.data(key).unwrap().uuid;
        let mut origin = None;
        let mut cloud_owner = None;
        let mut breaths = Vec::new();
        for tick in 1..=FLAME_TICKS {
            let mut events = Vec::new();
            fly(&mut store, key, &[], &mut rng, &mut events);
            for event in events {
                if let MobEvent::Breath { pos, owner } = event {
                    breaths.push(tick);
                    origin = Some(pos);
                    cloud_owner = owner;
                }
            }
        }
        assert_eq!(breaths.len(), 1, "one cloud per flaming phase");
        assert!(breaths[0] <= FLAME_WARMUP_TICKS + 1);
        let body = store.pos(key).unwrap();
        let yaw = store.data(key).unwrap().yaw;
        assert_eq!(origin, Some(breath_aim(body, yaw)));
        assert_eq!(cloud_owner, Some(dragon_uuid));
        assert_eq!(phase_of(&store, key), DragonPhase::Perched);
        // The fourth flaming phase ends the perch instead.
        let (mut store, key, mut rng) = perched(DragonPhase::PerchFlaming);
        mob::state_mut(&mut store, key).unwrap().perch_flames = 4;
        run(&mut store, key, &mut rng, &[], FLAME_TICKS);
        assert_eq!(phase_of(&store, key), DragonPhase::Takeoff);
    }

    #[test]
    fn landing_ends_in_the_scanning_perch() {
        let (mut store, key, mut rng) = perched(DragonPhase::LandingApproach);
        run(&mut store, key, &mut rng, &[], 1);
        assert_eq!(phase_of(&store, key), DragonPhase::Perched);
    }

    #[test]
    fn scanning_with_nobody_near_takes_off_on_tick_100_without_breathing() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        let breaths = run(&mut store, key, &mut rng, &[], SCAN_IDLE_TICKS - 1);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::Perched,
            "still scanning at 99"
        );
        run(&mut store, key, &mut rng, &[], 1);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::Takeoff,
            "gives up at 100"
        );
        assert!(breaths.is_empty());
        // Takeoff ends in the holding pattern once the dragon reaches the circle.
        let age = mob::state(&store, key).unwrap().wander_timer;
        let (circle, _) = position_at(age + 1);
        store.set_pos(key, circle);
        run(&mut store, key, &mut rng, &[], 1);
        assert_eq!(phase_of(&store, key), DragonPhase::Holding);
    }

    #[test]
    fn a_player_in_range_starts_the_attack_after_more_than_25_ticks_of_scanning() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        run(&mut store, key, &mut rng, &near_player(), SCAN_ATTACK_DELAY);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::Perched,
            "tick 25 still scans"
        );
        run(&mut store, key, &mut rng, &near_player(), 1);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::PerchAttacking,
            "tick 26 attacks"
        );
        assert_eq!(mob::state(&store, key).unwrap().dragon_phase_ticks, 0);
    }

    #[test]
    fn a_player_out_of_scan_range_does_not_start_an_attack() {
        let far = [Target {
            uuid: uuid::Uuid::from_u128(32),
            pos: PERCH_SPOT + Vec3::new(SCAN_RANGE + 5.0, 0.0, 0.0),
        }];
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        run(&mut store, key, &mut rng, &far, SCAN_IDLE_TICKS - 1);
        assert_eq!(phase_of(&store, key), DragonPhase::Perched);
        run(&mut store, key, &mut rng, &far, 1);
        // The scan gives up without attacking; a player within 150 blocks is charged.
        assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
    }

    #[test]
    fn a_player_who_leaves_before_the_delay_does_not_start_an_attack() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        run(&mut store, key, &mut rng, &near_player(), 20);
        run(&mut store, key, &mut rng, &[], SCAN_IDLE_TICKS - 20);
        assert_eq!(phase_of(&store, key), DragonPhase::Takeoff);
    }

    #[test]
    fn the_attack_roars_for_40_ticks_then_flames() {
        let (mut store, key, mut rng) = perched(DragonPhase::PerchAttacking);
        let breaths = run(&mut store, key, &mut rng, &near_player(), ATTACK_TICKS - 1);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::PerchAttacking,
            "tick 39"
        );
        assert!(breaths.is_empty(), "no breath while roaring");
        run(&mut store, key, &mut rng, &near_player(), 1);
        assert_eq!(phase_of(&store, key), DragonPhase::PerchFlaming, "tick 40");
    }

    #[test]
    fn a_flaming_phase_breathes_once_after_ten_ticks_then_scans_again() {
        let (mut store, key, mut rng) = perched(DragonPhase::PerchFlaming);
        mob::state_mut(&mut store, key).unwrap().perch_flames = 1;
        let breaths = run(&mut store, key, &mut rng, &near_player(), FLAME_TICKS - 1);
        assert_eq!(phase_of(&store, key), DragonPhase::PerchFlaming, "tick 199");
        run(&mut store, key, &mut rng, &near_player(), 1);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::Perched,
            "tick 200 scans again"
        );
        assert_eq!(mob::state(&store, key).unwrap().dragon_phase_ticks, 0);
        assert_eq!(breaths, vec![FLAME_WARMUP_TICKS]);
        assert_eq!(
            mob::state(&store, key).unwrap().perch_flames,
            1,
            "the flaming phase is counted"
        );
    }

    #[test]
    fn four_flaming_phases_end_the_perch_with_takeoff() {
        let (mut store, key, mut rng) = perched(DragonPhase::PerchAttacking);
        mob::state_mut(&mut store, key).unwrap().perch_flames = FLAME_ATTACKS - 1;
        run(&mut store, key, &mut rng, &near_player(), ATTACK_TICKS);
        assert_eq!(phase_of(&store, key), DragonPhase::PerchFlaming);
        assert_eq!(
            mob::state(&store, key).unwrap().perch_flames,
            FLAME_ATTACKS,
            "the fourth flaming phase is counted"
        );
        run(&mut store, key, &mut rng, &near_player(), FLAME_TICKS);
        assert_eq!(
            phase_of(&store, key),
            DragonPhase::Takeoff,
            "four flames end the perch"
        );
    }

    #[test]
    fn a_player_above_the_scan_box_does_not_start_an_attack() {
        let above = [Target {
            uuid: uuid::Uuid::from_u128(33),
            pos: PERCH_SPOT + Vec3::new(0.0, SCAN_RANGE_Y + 1.0, 0.0),
        }];
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        run(&mut store, key, &mut rng, &above, SCAN_IDLE_TICKS - 1);
        assert_eq!(phase_of(&store, key), DragonPhase::Perched);
        run(&mut store, key, &mut rng, &above, 1);
        // The scan gives up without attacking; a player within 150 blocks is charged.
        assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
    }

    #[test]
    fn a_player_who_stays_keeps_the_dragon_cycling_scan_attack_flame() {
        let (mut store, key, mut rng) = perched(DragonPhase::LandingApproach);
        let mut seen = Vec::new();
        for _ in 0..700 {
            run(&mut store, key, &mut rng, &near_player(), 1);
            let phase = phase_of(&store, key);
            if seen.last() != Some(&phase) {
                seen.push(phase);
            }
        }
        assert_eq!(
            seen[..5],
            [
                DragonPhase::Perched,
                DragonPhase::PerchAttacking,
                DragonPhase::PerchFlaming,
                DragonPhase::Perched,
                DragonPhase::PerchAttacking,
            ]
        );
    }

    #[test]
    fn a_strafe_shot_returns_to_the_holding_pattern() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        let target = Target {
            uuid: uuid::Uuid::from_u128(21),
            pos: Vec3::new(20.0, 80.0, 20.0),
        };
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.dragon_phase = DragonPhase::Strafe;
            state.target = Some(target.uuid);
        }
        let mut events = Vec::new();
        let mut returned = false;
        for _ in 0..60 {
            fly(&mut store, key, &[target], &mut rng, &mut events);
            if mob::state(&store, key).unwrap().dragon_phase == DragonPhase::Holding {
                returned = true;
                break;
            }
        }
        assert!(returned, "the strafe ends after its fireball");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MobEvent::DragonFireball { .. })),
            "the strafe fires at the player"
        );
    }

    #[test]
    fn losing_the_strafe_target_returns_to_holding() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::new(4);
        let (spot, _) = position_at(0);
        let key = store.insert(mob::new_mob(mob::MobKind::EnderDragon, spot, &mut rng));
        {
            let state = mob::state_mut(&mut store, key).unwrap();
            state.dragon_phase = DragonPhase::Strafe;
            state.target = Some(uuid::Uuid::from_u128(22));
        }
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(
            mob::state(&store, key).unwrap().dragon_phase,
            DragonPhase::Holding,
            "a strafe without its player cannot orbit"
        );
    }

    fn far_player(distance: f64) -> [Target; 1] {
        [Target {
            uuid: uuid::Uuid::from_u128(32),
            pos: PERCH_SPOT + Vec3::new(distance, 0.0, 0.0),
        }]
    }

    #[test]
    fn giving_up_the_scan_charges_a_player_within_150_blocks() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        let players = far_player(100.0);
        run(&mut store, key, &mut rng, &players, SCAN_IDLE_TICKS);
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::ChargePlayer);
        // The charge aims at where the player stood when the scan gave up.
        assert_eq!(state.dragon_charge_target, Some(players[0].pos));
        assert_eq!(state.dragon_phase_ticks, 0);
    }

    #[test]
    fn giving_up_the_scan_still_takes_off_when_the_player_is_beyond_150_blocks() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        run(
            &mut store,
            key,
            &mut rng,
            &far_player(151.0),
            SCAN_IDLE_TICKS,
        );
        assert_eq!(phase_of(&store, key), DragonPhase::Takeoff);
        assert_eq!(mob::state(&store, key).unwrap().dragon_charge_target, None);
    }

    #[test]
    fn a_charge_flies_at_the_fixed_target_then_recovers_for_10_ticks() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        let players = far_player(100.0);
        run(&mut store, key, &mut rng, &players, SCAN_IDLE_TICKS);
        assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
        // The player moves away; the charge keeps its original target.
        let moved = [Target {
            uuid: players[0].uuid,
            pos: players[0].pos + Vec3::new(0.0, 0.0, 30.0),
        }];
        let mut reached_at = None;
        for tick in 1..=200 {
            fly(&mut store, key, &moved, &mut rng, &mut Vec::new());
            let pos = store.pos(key).unwrap();
            if reached_at.is_none()
                && pos.distance_to_sqr(players[0].pos) < CHARGE_REACH * CHARGE_REACH
            {
                reached_at = Some(tick);
            }
            if phase_of(&store, key) == DragonPhase::Holding {
                // `timeSinceCharge` starts on the reaching tick and the phase ends on the tick
                // that finds it at 10, so holding resumes 10 ticks after the reaching tick.
                assert_eq!(tick, reached_at.unwrap() + CHARGE_RECOVERY_TICKS);
                assert_eq!(mob::state(&store, key).unwrap().dragon_charge_target, None);
                return;
            }
        }
        panic!("the charge never ended");
    }

    #[test]
    fn a_charge_dives_with_the_fly_speed_3_slope_clamp() {
        // 1 block across, 100 down: the slope clamp keeps the dive at 3 blocks per block.
        let step = clamp_slope(Vec3::new(1.0, -100.0, 0.0), CHARGE_FLY_SPEED);
        assert_eq!((step.x, step.y, step.z), (1.0, -3.0, 0.0));
        // Other phases would be held to 0.6; the charge is not.
        assert!(clamp_slope(Vec3::new(1.0, -100.0, 0.0), 0.6).y > step.y);
        // A shallow step and a purely vertical one are untouched.
        let shallow = Vec3::new(10.0, 4.0, 0.0);
        assert_eq!(clamp_slope(shallow, CHARGE_FLY_SPEED), shallow);
        let vertical = Vec3::new(0.0, -50.0, 0.0);
        assert_eq!(clamp_slope(vertical, CHARGE_FLY_SPEED), vertical);
        // In `fly`: a charge aimed almost straight down moves down steeply but never past
        // `MAX_SPEED`.
        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        let spot = store.pos(key).unwrap();
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(spot + Vec3::new(2.0, -140.0, 0.0));
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        let moved = store.pos(key).unwrap() - spot;
        assert!(moved.y < -1.0, "the dive should be steep: {moved:?}");
        assert!(moved.length_sqr() <= MAX_SPEED * MAX_SPEED + 1e-9);
    }

    #[test]
    fn a_charge_flies_through_blocks_and_only_distance_ends_it() {
        // `noPhysics` means vanilla never sees a collision, so nothing but distance starts the
        // recovery: far inside the 10..150 band the phase keeps going tick after tick.
        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        let spot = store.pos(key).unwrap();
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(spot + Vec3::new(120.0, 0.0, 0.0));
        for _ in 0..20 {
            fly(&mut store, key, &[], &mut rng, &mut Vec::new());
            assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
            assert_eq!(mob::state(&store, key).unwrap().dragon_charge_recovery, 0);
        }
        // A target already beyond 150 blocks starts the 10-tick recovery at once.
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(store.pos(key).unwrap() + Vec3::new(500.0, 0.0, 0.0));
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(mob::state(&store, key).unwrap().dragon_charge_recovery, 1);
        for _ in 0..CHARGE_RECOVERY_TICKS {
            assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
            fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        }
        assert_eq!(phase_of(&store, key), DragonPhase::Holding);
    }

    #[test]
    fn a_charge_without_a_target_aborts_to_holding() {
        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(phase_of(&store, key), DragonPhase::Holding);
    }

    #[test]
    fn the_charge_phase_round_trips_through_its_saved_name() {
        assert_eq!(
            DragonPhase::from_name(DragonPhase::ChargePlayer.name()),
            Some(DragonPhase::ChargePlayer)
        );
    }

    #[test]
    fn the_scan_turns_toward_a_player_more_than_ten_degrees_off_the_heading() {
        let pos = PERCH_SPOT;
        // Yaw 0 faces +z; the player stands on +x, 90 degrees off.
        let player = pos + Vec3::new(15.0, 0.0, 0.0);
        let (yaw, rate) = scan_turn(0.0, 0.0, pos, player);
        // Facing +z toward +x means a negative yaw (vanilla yaw = -atan2(dx, dz)).
        assert!(
            yaw < 0.0 && rate < 0.0,
            "turned the wrong way: {yaw} {rate}"
        );
        // Repeated ticks converge on the player and then stop turning inside 10 degrees.
        let (mut yaw, mut rate) = (0.0_f32, 0.0_f32);
        for _ in 0..400 {
            (yaw, rate) = scan_turn(yaw, rate, pos, player);
        }
        assert!((yaw + 90.0).abs() < 15.0, "settled at {yaw}");
    }

    #[test]
    fn the_scan_does_not_turn_when_already_facing_the_player() {
        let pos = PERCH_SPOT;
        let player = pos + Vec3::new(0.0, 0.0, 15.0);
        assert_eq!(scan_turn(0.0, 0.0, pos, player), (0.0, 0.0));
        // Five degrees off is still inside the threshold.
        assert_eq!(scan_turn(5.0, 0.0, pos, player), (5.0, 0.0));
    }

    #[test]
    fn a_perched_scan_with_a_player_turns_the_dragon_and_keeps_the_turn() {
        let (mut store, key, mut rng) = perched(DragonPhase::Perched);
        store.data_mut(key).unwrap().yaw = 0.0;
        run(&mut store, key, &mut rng, &near_player(), 10);
        let yaw = store.data(key).unwrap().yaw;
        assert!(yaw < 0.0, "the dragon should turn toward +x, yaw {yaw}");
        // The attack and flame phases keep that yaw instead of snapping back to the circle.
        let (mut store, key, mut rng) = perched(DragonPhase::PerchAttacking);
        store.data_mut(key).unwrap().yaw = 33.0;
        run(&mut store, key, &mut rng, &near_player(), 5);
        assert_eq!(store.data(key).unwrap().yaw, 33.0);
    }

    #[test]
    fn the_breath_aims_where_the_head_points_not_under_the_perch() {
        let pos = Vec3::new(0.0, 68.0, 0.0);
        // Vanilla sitting head is 6.5 forward and -1.0 in y (height 1.0 -> mid height -0.5);
        // the breath cloud forms 2.5 further forward (9.0 total) at head.getY(0.5) = 67.5.
        let head = sitting_head_pos(pos, 0.0);
        assert!(head.x.abs() < 1e-9 && (head.z - 6.5).abs() < 1e-9);
        assert_eq!(head.y, 67.0);
        let aim = breath_aim(pos, 0.0);
        assert!(aim.x.abs() < 1e-9 && (aim.z - 9.0).abs() < 1e-9);
        assert_eq!(aim.y, 67.5);
        // Yaw -90: heading +x.
        let aim = breath_aim(pos, -90.0);
        assert!((aim.x - 9.0).abs() < 1e-9 && aim.z.abs() < 1e-9);
        assert_eq!(aim.y, 67.5);
        // The perched flame event carries the aim point and owner for the dragon's actual yaw.
        let (mut store, key, mut rng) = perched(DragonPhase::PerchFlaming);
        store.data_mut(key).unwrap().yaw = -90.0;
        let dragon_uuid = store.data(key).unwrap().uuid;
        let mut events = Vec::new();
        for _ in 0..FLAME_WARMUP_TICKS {
            fly(&mut store, key, &near_player(), &mut rng, &mut events);
        }
        let Some(MobEvent::Breath { pos: cloud, owner }) = events
            .iter()
            .find(|event| matches!(event, MobEvent::Breath { .. }))
        else {
            panic!("no breath event");
        };
        assert!(
            (cloud.x - 9.0).abs() < 1e-6 && cloud.z.abs() < 0.5,
            "cloud at {cloud:?}"
        );
        assert_eq!(cloud.y, 67.5);
        assert_eq!(*owner, Some(dragon_uuid));
    }

    #[test]
    fn the_breath_cloud_sits_on_the_first_block_below_the_head() {
        // Ground at y = 60: air above it down to 61.
        assert_eq!(breath_floor(71.0, 0, |y| y > 60), 61.0);
        // Open void falls back to the head's own height, one above it.
        assert_eq!(breath_floor(71.0, 0, |_| true), 72.0);
        // Standing in a block already rests on it.
        assert_eq!(breath_floor(71.0, 0, |_| false), 72.0);
    }

    #[test]
    fn horizontal_collision_during_charge_starts_recovery_and_exits_to_holding() {
        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        let spot = store.pos(key).unwrap();
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(spot + Vec3::new(80.0, 0.0, 0.0));
        store
            .flags_mut(key)
            .unwrap()
            .set(Flags::HORIZONTAL_COLLISION, true);

        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
        assert_eq!(mob::state(&store, key).unwrap().dragon_charge_recovery, 1);
        assert!(
            !store.flags(key).unwrap().has(Flags::HORIZONTAL_COLLISION),
            "noPhysics movement clears horizontal collision after doServerTick reads it"
        );

        for tick in 1..CHARGE_RECOVERY_TICKS {
            fly(&mut store, key, &[], &mut rng, &mut Vec::new());
            assert_eq!(
                phase_of(&store, key),
                DragonPhase::ChargePlayer,
                "still recovering at tick {tick}"
            );
        }
        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Holding);
        assert_eq!(state.dragon_charge_target, None);
        assert_eq!(state.dragon_charge_recovery, 0);
    }

    #[test]
    fn vertical_collision_during_charge_starts_recovery_and_exits_to_holding() {
        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        let spot = store.pos(key).unwrap();
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(spot + Vec3::new(80.0, 0.0, 0.0));
        store
            .flags_mut(key)
            .unwrap()
            .set(Flags::VERTICAL_COLLISION, true);

        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
        assert_eq!(mob::state(&store, key).unwrap().dragon_charge_recovery, 1);
        assert!(!store.flags(key).unwrap().has(Flags::VERTICAL_COLLISION));

        for _ in 0..CHARGE_RECOVERY_TICKS {
            assert_eq!(phase_of(&store, key), DragonPhase::ChargePlayer);
            fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        }
        let state = mob::state(&store, key).unwrap();
        assert_eq!(state.dragon_phase, DragonPhase::Holding);
        assert_eq!(state.dragon_charge_target, None);
        assert_eq!(state.dragon_charge_recovery, 0);
    }

    #[test]
    fn a_dragon_spawns_with_no_physics_and_solid_block_collision_exits_charge_when_disabled() {
        struct WallAtX10;
        impl crate::physics::BlockGetter for WallAtX10 {
            fn block_state(&self, x: i32, _y: i32, _z: i32) -> Option<u16> {
                if x >= 10 {
                    Some(ferrite_data::blocks::default_state("minecraft:obsidian").unwrap_or(1))
                } else {
                    Some(0)
                }
            }
        }

        let (mut store, key, mut rng) = perched(DragonPhase::ChargePlayer);
        assert!(
            store.flags(key).unwrap().has(Flags::NO_PHYSICS),
            "vanilla EnderDragon spawns with noPhysics = true"
        );
        // With noPhysics disabled, moving into a solid wall sets HORIZONTAL_COLLISION, which
        // DragonChargePlayerPhase then consumes on the next fly tick.
        store.flags_mut(key).unwrap().set(Flags::NO_PHYSICS, false);
        let spot = Vec3::new(1.5, 68.0, 0.0);
        store.set_pos(key, spot);
        mob::state_mut(&mut store, key)
            .unwrap()
            .dragon_charge_target = Some(spot + Vec3::new(80.0, 0.0, 0.0));

        crate::physics::move_entity(&mut store, key, Vec3::new(2.0, 0.0, 0.0), &WallAtX10);
        assert!(store.flags(key).unwrap().has(Flags::HORIZONTAL_COLLISION));

        fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        assert_eq!(mob::state(&store, key).unwrap().dragon_charge_recovery, 1);
        for _ in 0..CHARGE_RECOVERY_TICKS {
            fly(&mut store, key, &[], &mut rng, &mut Vec::new());
        }
        assert_eq!(phase_of(&store, key), DragonPhase::Holding);
    }
}
