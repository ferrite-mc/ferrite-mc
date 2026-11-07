//! Minimal living-mob framework: the first passive animals, their health, damage and
//! knockback, death, fall damage and vanilla despawn rules.
//!
//! Kinds are resolved from the vanilla entity data (`ferrite_data::entity_props`), so
//! dimensions, tracking, attributes and loot table names come from the 26.3 generated
//! reports instead of being restated here. Active mob behaviour (goals, spawning) lives in
//! [`crate::ai`] and [`crate::spawn`].

use crate::id::{EntityKey, allocate_id, random_uuid};
use crate::rng::Rng;
use crate::store::{EntityData, EntityRecord, EntityStore, Flags};
use ferrite_data::entity_props::{self, EntityProps};
use ferrite_math::Vec3;

/// Ticks a mob ignores further hits after being hurt (vanilla `invulnerableTime / 2`).
pub const HURT_COOLDOWN: u16 = 10;
/// Vanilla creature despawn distance (`MobCategory.CREATURE.getDespawnDistance()`).
pub const DESPAWN_DISTANCE: f64 = 128.0;
/// Inside this radius a mob's no-action timer resets (`getNoDespawnDistance()`).
pub const NO_DESPAWN_DISTANCE: f64 = 32.0;
/// Ticks without a nearby player before the random despawn roll starts.
pub const NO_ACTION_TICKS: u32 = 600;
/// How long a wild wolf stays angry (vanilla 20 to 39 seconds, the shorter value here).
pub const WOLF_ANGER_TICKS: u16 = 400;
/// Ticks a passive mob flees after being hurt (vanilla panic goal duration is 100-200).
pub const PANIC_TICKS: u16 = 100;

/// The mobs this server knows how to simulate: the first passive animals and the hostile
/// monsters of the overworld.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobKind {
    Chicken,
    Cow,
    Pig,
    Sheep,
    Zombie,
    Skeleton,
    Spider,
    Creeper,
    Enderman,
    Villager,
    EnderDragon,
    Wolf,
    Cat,
    Parrot,
    Horse,
    Donkey,
    Mule,
    Llama,
}

impl MobKind {
    /// The passive kinds, in a fixed order for creature spawning.
    pub const ALL: [Self; 4] = [Self::Chicken, Self::Cow, Self::Pig, Self::Sheep];
    /// The hostile kinds, in a fixed order for monster spawning.
    pub const HOSTILE: [Self; 5] = [
        Self::Zombie,
        Self::Skeleton,
        Self::Spider,
        Self::Creeper,
        Self::Enderman,
    ];
    /// Every modelled kind.
    pub const ALL_MOBS: [Self; 18] = [
        Self::Chicken,
        Self::Cow,
        Self::Pig,
        Self::Sheep,
        Self::Zombie,
        Self::Skeleton,
        Self::Spider,
        Self::Creeper,
        Self::Enderman,
        Self::Villager,
        Self::EnderDragon,
        Self::Wolf,
        Self::Cat,
        Self::Parrot,
        Self::Horse,
        Self::Donkey,
        Self::Mule,
        Self::Llama,
    ];

    /// Vanilla entity type id, e.g. `"minecraft:cow"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Chicken => "minecraft:chicken",
            Self::Cow => "minecraft:cow",
            Self::Pig => "minecraft:pig",
            Self::Sheep => "minecraft:sheep",
            Self::Zombie => "minecraft:zombie",
            Self::Skeleton => "minecraft:skeleton",
            Self::Spider => "minecraft:spider",
            Self::Creeper => "minecraft:creeper",
            Self::Enderman => "minecraft:enderman",
            Self::Villager => "minecraft:villager",
            Self::EnderDragon => "minecraft:ender_dragon",
            Self::Wolf => "minecraft:wolf",
            Self::Cat => "minecraft:cat",
            Self::Parrot => "minecraft:parrot",
            Self::Horse => "minecraft:horse",
            Self::Donkey => "minecraft:donkey",
            Self::Mule => "minecraft:mule",
            Self::Llama => "minecraft:llama",
        }
    }

    /// Whether an owner can tame, seat and call this kind (`TamableAnimal`).
    pub fn is_tameable(self) -> bool {
        matches!(self, Self::Wolf | Self::Cat | Self::Parrot)
    }

    /// Horses, donkeys and mules: rideable, tamed by riding and saddled (`AbstractHorse`).
    pub fn is_equine(self) -> bool {
        matches!(self, Self::Horse | Self::Donkey | Self::Mule | Self::Llama)
    }

    /// Whether this kind hunts players (vanilla `Monster`/`Enemy`).
    pub fn is_hostile(self) -> bool {
        matches!(
            self,
            Self::Zombie | Self::Skeleton | Self::Spider | Self::Creeper | Self::Enderman
        )
    }

    /// Vanilla `MobCategory` name from the generated entity data.
    pub fn category(self) -> &'static str {
        self.props().category
    }

    /// Melee damage, from the type's default `minecraft:attack_damage` attribute.
    pub fn attack_damage(self) -> f32 {
        entity_props::default_attribute(self.entity_type(), "attack_damage").unwrap_or(2.0) as f32
    }

    /// How far the mob notices and follows a player (`follow_range` attribute).
    pub fn follow_range(self) -> f64 {
        entity_props::default_attribute(self.entity_type(), "follow_range")
            .unwrap_or(16.0)
            .clamp(8.0, 48.0)
    }

    /// Vanilla melee reach of the modelled monsters.
    pub fn attack_reach(self) -> f64 {
        match self {
            Self::Zombie | Self::Spider | Self::Enderman => 2.5,
            _ => 2.0,
        }
    }

    /// Vanilla properties of this entity type.
    pub fn props(self) -> &'static EntityProps {
        entity_props::by_name(self.name()).expect("the vanilla data knows every mob kind")
    }

    /// Global protocol id of the entity type.
    pub fn entity_type(self) -> u16 {
        self.props().id
    }

    /// Base maximum health, from the type's default `minecraft:max_health` attribute.
    pub fn max_health(self) -> f32 {
        entity_props::default_attribute(self.entity_type(), "max_health").unwrap_or(10.0) as f32
    }

    /// Base movement speed, from the type's default `minecraft:movement_speed` attribute.
    pub fn movement_speed(self) -> f64 {
        entity_props::default_attribute(self.entity_type(), "movement_speed").unwrap_or(0.25)
    }

    /// Jump impulse, from the type's default `minecraft:jump_strength` attribute.
    pub fn jump_strength(self) -> f64 {
        entity_props::default_attribute(self.entity_type(), "jump_strength").unwrap_or(0.42)
    }

    /// The entity loot table, e.g. `minecraft:entities/cow`.
    pub fn loot_table(self) -> &'static str {
        match self {
            Self::Chicken => "minecraft:entities/chicken",
            Self::Cow => "minecraft:entities/cow",
            Self::Pig => "minecraft:entities/pig",
            Self::Sheep => "minecraft:entities/sheep",
            Self::Zombie => "minecraft:entities/zombie",
            Self::Skeleton => "minecraft:entities/skeleton",
            Self::Spider => "minecraft:entities/spider",
            Self::Creeper => "minecraft:entities/creeper",
            Self::Enderman => "minecraft:entities/enderman",
            // Villagers drop their held items only, which this server does not model.
            Self::Villager => "",
            // The dragon drops experience and the egg, handled by the fight.
            Self::EnderDragon => "",
            // A wolf drops nothing.
            Self::Wolf => "",
            Self::Cat => "minecraft:entities/cat",
            Self::Parrot => "minecraft:entities/parrot",
            Self::Horse => "minecraft:entities/horse",
            Self::Donkey => "minecraft:entities/donkey",
            Self::Mule => "minecraft:entities/mule",
            Self::Llama => "minecraft:entities/llama",
        }
    }

    /// The kind of an entity type id, when it is one of the modelled mobs.
    pub fn from_entity_type(kind: u16) -> Option<Self> {
        Self::ALL_MOBS
            .into_iter()
            .find(|mob| mob.entity_type() == kind)
    }

    /// The kind of a name, with or without the namespace.
    pub fn from_name(name: &str) -> Option<Self> {
        entity_props::by_name(name).and_then(|props| Self::from_entity_type(props.id))
    }

    /// English display name, as used by `/summon` feedback.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Chicken => "Chicken",
            Self::Cow => "Cow",
            Self::Pig => "Pig",
            Self::Sheep => "Sheep",
            Self::Zombie => "Zombie",
            Self::Skeleton => "Skeleton",
            Self::Spider => "Spider",
            Self::Creeper => "Creeper",
            Self::Enderman => "Enderman",
            Self::Villager => "Villager",
            Self::EnderDragon => "Ender Dragon",
            Self::Wolf => "Wolf",
            Self::Cat => "Cat",
            Self::Parrot => "Parrot",
            Self::Horse => "Horse",
            Self::Donkey => "Donkey",
            Self::Mule => "Mule",
            Self::Llama => "Llama",
        }
    }

    /// Vanilla `Animal.removeWhenFarAway` is false; monsters despawn when far away.
    pub fn despawns_when_far(self) -> bool {
        self.is_hostile()
    }
}

/// Living state of a mob; travels with the entity across region hand-offs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobState {
    pub kind: MobKind,
    pub health: f32,
    /// Ticks left before the mob can be hurt again.
    pub hurt_time: u16,
    /// Ticks since a player was last close enough to keep the mob active.
    pub no_action_time: u32,
    /// Blocks fallen since leaving the ground (vanilla `fallDistance`).
    pub fall_distance: f32,
    /// Vanilla `removeWhenFarAway`; animals keep this false.
    pub despawns: bool,
    /// Whether the death loot was already rolled and spawned.
    pub loot_dropped: bool,
    /// Vanilla `NoAI`: no goals, so the mob never walks, looks or attacks; gravity still applies.
    pub no_ai: bool,
    /// Ticks the mob keeps burning (`Entity.remainingFireTicks`); it takes 1 damage a second.
    pub fire: u16,
    /// Ticks until the wander goal picks a new direction; zero picks now.
    pub wander_timer: u32,
    /// Yaw the wander goal walks along, in degrees.
    pub wander_yaw: f32,
    /// Whether the wander goal wants to move during this interval.
    pub moving: bool,
    /// Player this mob is hunting, for hostile kinds (vanilla `Mob.target`).
    pub target: Option<uuid::Uuid>,
    /// Ticks until the next melee attack (`Mob.attackStrengthTicker`-like cooldown).
    pub attack_cooldown: u16,
    /// Ticks of panic left after being hurt (vanilla `PanicGoal`).
    pub panic: u16,
    /// Where the panic flee goal runs from.
    pub panic_from: Option<Vec3>,
    /// Creeper swell counter (`Creeper.swell`).
    pub swell: u16,
    /// Ticks until the skeleton's next arrow.
    pub bow_timer: u16,
    /// Path waypoints as block positions (`PathNavigation.getPath`).
    pub path: [[i32; 3]; crate::path::MAX_PATH],
    pub path_len: u8,
    pub path_index: u8,
    /// Ticks until the path is recomputed.
    pub path_timer: u16,
    /// Vanilla `AgeableMob.age`: negative for a baby (counts up to 0), positive for the breeding
    /// cooldown (counts down).
    pub age: i32,
    /// Ticks left in love (`Animal.inLove`).
    pub love: u16,
    /// The player who fed the animal, for the breeding advancement.
    pub love_cause: Option<uuid::Uuid>,
    /// Ticks spent next to the mate.
    pub breed_timer: u16,
    /// The player a tamed animal belongs to (`TamableAnimal.owner`).
    pub owner: Option<uuid::Uuid>,
    /// The player holding this animal on a lead, and that player's entity id for the client.
    pub leash: Option<(uuid::Uuid, i32)>,
    /// The llama this one trails in a caravan (`Llama.caravanHead`).
    pub caravan: Option<uuid::Uuid>,
    /// Whether a tamed animal was told to sit.
    pub sitting: bool,
    /// Whether a tamed animal is walking to its owner (`FollowOwnerGoal`).
    pub following: bool,
    /// Ticks a wild wolf stays angry at its target (`NeutralMob` anger time).
    pub anger: u16,
    /// A tame horse wears a saddle and can be steered.
    pub saddled: bool,
    /// Item id of the horse armour it wears (0 for none).
    pub armor: u16,
    /// A donkey or mule carries a chest (`AbstractChestedHorse.hasChest`).
    pub chest: bool,
    /// Armour points of the worn horse armour (`Attributes.ARMOR`).
    pub armor_points: f32,
    /// Inherited `movement_speed` and `jump_strength` of an equine (0 when not rolled).
    pub speed: f32,
    pub jump: f32,
    /// How much a wild horse has come to accept riders (`AbstractHorse.temper`).
    pub temper: i32,
}

impl MobState {
    /// A full-health mob of `kind`.
    pub fn new(kind: MobKind) -> Self {
        Self {
            kind,
            health: kind.max_health(),
            hurt_time: 0,
            no_action_time: 0,
            fall_distance: 0.0,
            despawns: kind.despawns_when_far(),
            loot_dropped: false,
            no_ai: false,
            fire: 0,
            wander_timer: 0,
            wander_yaw: 0.0,
            moving: false,
            target: None,
            attack_cooldown: 0,
            panic: 0,
            panic_from: None,
            swell: 0,
            bow_timer: 0,
            path: [[0; 3]; crate::path::MAX_PATH],
            path_len: 0,
            path_index: 0,
            path_timer: 0,
            age: 0,
            love: 0,
            love_cause: None,
            breed_timer: 0,
            owner: None,
            sitting: false,
            following: false,
            anger: 0,
            saddled: false,
            armor: 0,
            chest: false,
            leash: None,
            caravan: None,
            armor_points: 0.0,
            speed: 0.0,
            jump: 0.0,
            temper: 0,
        }
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }
}

/// The outcome of one hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hurt {
    /// The hit landed and the mob survived with this much health left.
    Survived { health: f32 },
    /// The hurt cooldown swallowed the hit.
    Immune,
    /// The hit was lethal; the mob is flagged for removal.
    Died,
}

/// A new mob at `pos` with a random yaw, like vanilla's `Mob.finalizeSpawn`-created entity.
pub fn new_mob(kind: MobKind, pos: Vec3, rng: &mut Rng) -> EntityRecord {
    let props = kind.props();
    let mut data = EntityData::new(
        allocate_id(),
        random_uuid(),
        kind.entity_type(),
        f64::from(props.width),
        f64::from(props.height),
    );
    data.yaw = rng.next_f32() * 360.0;
    data.mob = Some(MobState::new(kind));
    if kind.is_animal()
        && let Some(state) = data.mob.as_ref()
    {
        crate::breed::define_metadata(&mut data.metadata, state);
    }
    if kind.is_tameable()
        && let Some(state) = data.mob.as_ref()
    {
        crate::tame::define_metadata(&mut data.metadata, state);
    }
    if kind.is_equine()
        && let Some(state) = data.mob.as_ref()
    {
        crate::horse::define_metadata(&mut data.metadata, state);
        // `AbstractHorse.randomizeAttributes`: speed 0.1125 to 0.3375, jump 0.4 to 1.0 and 15 to
        // 30 health, each from several rolls so the middle is likelier.
        if let Some(mob) = data.mob.as_mut() {
            mob.speed =
                (0.45 + rng.next_f32() * 0.3 + rng.next_f32() * 0.3 + rng.next_f32() * 0.3) * 0.25;
            mob.jump = 0.4 + rng.next_f32() * 0.2 + rng.next_f32() * 0.2 + rng.next_f32() * 0.2;
            mob.health = 15.0 + rng.next_int(8) as f32 + rng.next_int(9) as f32;
        }
        if kind == MobKind::Llama {
            // `Llama.finalizeSpawn`: a random strength (1 to 5) and one of four coats.
            data.metadata.set(
                crate::horse::STRENGTH_INDEX,
                ferrite_protocol::packets::play::MetaValue::Int(1 + rng.next_int(5)),
            );
            data.metadata.set(
                crate::horse::LLAMA_VARIANT_INDEX,
                ferrite_protocol::packets::play::MetaValue::Int(rng.next_int(4)),
            );
        }
        if kind == MobKind::Horse {
            // A random coat (7 colours) with a random marking (5 of them), like
            // `Horse.finalizeSpawn`.
            let coat = rng.next_int(7) | (rng.next_int(5) << 8);
            data.metadata.set(
                crate::horse::VARIANT_INDEX,
                ferrite_protocol::packets::play::MetaValue::Int(coat),
            );
        }
    }
    if kind == MobKind::Villager {
        let state = crate::villager::VillagerState::new(
            crate::villager::VillagerType::Plains,
            [pos.x, pos.y, pos.z],
        );
        crate::villager::define_metadata(&mut data.metadata, &state);
        data.villager = Some(state);
    }
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags: Flags::default(),
        data,
    }
}

/// Living state of an entity, if it is a mob.
pub fn state(store: &EntityStore, key: EntityKey) -> Option<&MobState> {
    store.data(key)?.mob.as_ref()
}

/// Mutable living state of an entity, if it is a mob.
pub fn state_mut(store: &mut EntityStore, key: EntityKey) -> Option<&mut MobState> {
    store.data_mut(key)?.mob.as_mut()
}

/// Whether a live, non-removed mob of one of the modelled kinds is at `key`.
pub fn is_mob(store: &EntityStore, key: EntityKey) -> bool {
    store.data(key).is_some_and(|data| data.mob.is_some())
        && store
            .flags(key)
            .is_some_and(|flags| !flags.has(Flags::REMOVED))
}

/// Applies vanilla `LivingEntity.hurt` rules, simplified to a half-second hurt cooldown:
/// knocks the mob away from `source` and flags the entity for removal when health runs out.
pub fn hurt(
    store: &mut EntityStore,
    key: EntityKey,
    amount: f32,
    source: Option<Vec3>,
) -> Option<Hurt> {
    if !amount.is_finite() || amount <= 0.0 {
        return None;
    }
    if !is_mob(store, key) {
        return None;
    }
    let state = *state(store, key)?;
    if state.is_dead() {
        return None;
    }
    if state.hurt_time > 0 {
        return Some(Hurt::Immune);
    }
    // `CombatRules.getDamageAfterAbsorb`: armour takes away up to 80% of a hit.
    let amount = if state.armor_points > 0.0 {
        let reduction = (state.armor_points / 5.0)
            .max(state.armor_points - amount / 2.0)
            .clamp(0.0, 20.0);
        amount * (1.0 - reduction / 25.0)
    } else {
        amount
    };
    let health = (state.health - amount).max(0.0);
    {
        let mob = state_mut(store, key)?;
        mob.health = health;
        mob.hurt_time = HURT_COOLDOWN;
    }
    if let Some(source) = source {
        knockback(store, key, source);
    }
    // Passive mobs panic and flee from the attacker (vanilla `PanicGoal`); monsters keep
    // hunting (the region sets their target from the attacker).
    if let Some(source) = source
        && let Some(mob) = state_mut(store, key)
        && !mob.kind.is_hostile()
        && mob.kind != MobKind::Wolf
    {
        mob.panic = PANIC_TICKS;
        mob.panic_from = Some(source);
    }
    if health <= 0.0 {
        kill(store, key);
        Some(Hurt::Died)
    } else {
        Some(Hurt::Survived { health })
    }
}

/// Makes a hostile mob hunt `player` after being attacked (vanilla `Mob.setTarget` in
/// `LivingEntity.hurt`'s retaliation hook).
pub fn retaliate(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid) -> bool {
    let Some(mob) = state_mut(store, key) else {
        return false;
    };
    let kind_is_wolf = mob.kind == MobKind::Wolf;
    if matches!(mob.kind, MobKind::Wolf | MobKind::Llama) {
        // A wild wolf (or llama) turns on whoever hurt it; a tame one never does.
        if mob.owner.is_some() {
            return false;
        }
        mob.target = Some(player);
        mob.anger = WOLF_ANGER_TICKS;
        if kind_is_wolf {
            alert_pack(store, key, player);
        }
        return true;
    }
    if !mob.kind.is_hostile() {
        return false;
    }
    mob.target = Some(player);
    true
}

/// Wild wolves within this distance join a wolf that was attacked (`Wolf.hurt` alerts the pack).
const PACK_ALERT_RADIUS: f64 = 16.0;

/// Every other wild wolf near `key` turns on `player` as well. shortcut: vanilla uses a box
/// around the wolf and skips wolves that cannot see the target; this is a plain radius.
fn alert_pack(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid) {
    let Some(center) = store.pos(key) else {
        return;
    };
    let near: Vec<EntityKey> = store
        .keys()
        .iter()
        .copied()
        .filter(|&other| other != key)
        .filter(|&other| {
            store.pos(other).is_some_and(|p| {
                let (dx, dy, dz) = (p.x - center.x, p.y - center.y, p.z - center.z);
                dx * dx + dy * dy + dz * dz <= PACK_ALERT_RADIUS * PACK_ALERT_RADIUS
            }) && state(store, other).is_some_and(|m| m.kind == MobKind::Wolf && m.owner.is_none())
        })
        .collect();
    for other in near {
        if let Some(mob) = state_mut(store, other) {
            mob.target = Some(player);
            mob.anger = WOLF_ANGER_TICKS;
        }
    }
}

/// Kills a mob outright (`/kill`, lethal effects), bypassing the hurt cooldown.
pub fn kill(store: &mut EntityStore, key: EntityKey) -> bool {
    if !is_mob(store, key) {
        return false;
    }
    if let Some(mob) = state_mut(store, key) {
        mob.health = 0.0;
    }
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::REMOVED, true);
    }
    true
}

/// Vanilla `LivingEntity.knockback`: 0.4 blocks/tick away from the attacker, plus 0.4 upward
/// while the mob stands on the ground.
pub fn knockback(store: &mut EntityStore, key: EntityKey, source: Vec3) {
    let Some(pos) = store.pos(key) else { return };
    let mut dx = pos.x - source.x;
    let mut dz = pos.z - source.z;
    if dx * dx + dz * dz < 1.0e-5 {
        // Vanilla draws a small random direction; a fixed diagonal keeps it deterministic.
        dx = 0.01;
        dz = 0.01;
    }
    let length = (dx * dx + dz * dz).sqrt();
    let mut vel = store.vel(key).unwrap_or_default();
    vel.x = dx / length * 0.4;
    vel.z = dz / length * 0.4;
    if store
        .flags(key)
        .is_some_and(|flags| flags.has(Flags::ON_GROUND))
    {
        vel.y = vel.y.max(0.4);
    }
    store.set_vel(key, vel);
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::NEEDS_SYNC, true);
    }
}

/// Vanilla `LivingEntity.checkFallDamage`, applied with the type's `safe_fall_distance`.
pub fn apply_fall_damage(store: &mut EntityStore, key: EntityKey, distance: f32) -> Option<Hurt> {
    let kind = state(store, key)?.kind;
    let safe = entity_props::default_attribute(kind.entity_type(), "safe_fall_distance")
        .unwrap_or(3.0) as f32;
    let damage = (distance - safe).floor();
    if damage <= 0.0 {
        return None;
    }
    hurt(store, key, damage, None)
}

/// Advances the per-mob clocks: `tickCount`, the hurt cooldown and the no-action timer.
pub fn tick_clock(store: &mut EntityStore, key: EntityKey) {
    let Some(data) = store.data_mut(key) else {
        return;
    };
    data.tick_count = data.tick_count.saturating_add(1);
    let mut burn = false;
    if let Some(mob) = &mut data.mob {
        mob.hurt_time = mob.hurt_time.saturating_sub(1);
        mob.no_action_time = mob.no_action_time.saturating_add(1);
        if mob.fire > 0 {
            mob.fire -= 1;
            burn = mob.fire % 20 == 0;
        }
    }
    if burn {
        hurt(store, key, 1.0, None);
    }
}

/// Sets a mob on fire for `ticks` (vanilla `Entity.igniteForSeconds`); it keeps the longer time.
pub fn ignite(store: &mut EntityStore, key: EntityKey, ticks: u16) {
    if let Some(mob) = state_mut(store, key) {
        mob.fire = mob.fire.max(ticks);
    }
}

/// Vanilla `Mob.checkDespawn` for the modelled kinds.
///
/// Passive animals never satisfy `removeWhenFarAway`, so only the peaceful rule (which they
/// also do not use), the beyond-128-blocks rule and the 600-tick/no-player random roll can
/// remove them; [`MobState::despawns`] is kept as the knob future hostile kinds will set.
pub fn check_despawn(
    store: &mut EntityStore,
    key: EntityKey,
    players: &[Vec3],
    peaceful: bool,
    rng: &mut Rng,
) -> bool {
    let Some(mob) = state(store, key).copied() else {
        return false;
    };
    if mob.is_dead() || !is_mob(store, key) {
        return false;
    }
    if peaceful {
        // `Mob.shouldDespawnInPeaceful` is overridden only by monsters.
        return mob.kind.is_hostile() && remove(store, key);
    }
    if !mob.despawns {
        return false;
    }
    let nearest = players
        .iter()
        .map(|player| {
            let dx = player.x - store.pos(key).unwrap_or_default().x;
            let dz = player.z - store.pos(key).unwrap_or_default().z;
            dx * dx + dz * dz
        })
        .fold(f64::INFINITY, f64::min);
    let no_despawn = NO_DESPAWN_DISTANCE * NO_DESPAWN_DISTANCE;
    if nearest > DESPAWN_DISTANCE * DESPAWN_DISTANCE {
        return remove(store, key);
    }
    if mob.no_action_time > NO_ACTION_TICKS && nearest > no_despawn && rng.next_int(800) == 0 {
        return remove(store, key);
    }
    if nearest < no_despawn
        && let Some(mob) = state_mut(store, key)
    {
        mob.no_action_time = 0;
    }
    false
}

fn remove(store: &mut EntityStore, key: EntityKey) -> bool {
    match store.flags_mut(key) {
        Some(flags) => {
            flags.set(Flags::REMOVED, true);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(kind: MobKind) -> (EntityStore, EntityKey) {
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(kind, Vec3::new(0.5, 64.0, 0.5), &mut Rng::new(1)));
        (store, key)
    }

    #[test]
    fn kinds_match_the_vanilla_entity_data() {
        assert_eq!(MobKind::Cow.entity_type(), 30);
        for kind in MobKind::ALL {
            assert_eq!(MobKind::from_name(kind.name()), Some(kind));
            assert_eq!(MobKind::from_entity_type(kind.entity_type()), Some(kind));
            assert!(kind.max_health() > 0.0);
            assert!(kind.movement_speed() > 0.0);
            assert_eq!(kind.props().category, "creature");
            assert!(kind.loot_table().starts_with("minecraft:entities/"));
            assert!(!kind.despawns_when_far());
        }
        assert_eq!(MobKind::Cow.max_health(), 10.0);
        assert_eq!(MobKind::Chicken.max_health(), 4.0);
        assert_eq!(MobKind::from_name("villager"), Some(MobKind::Villager));
        assert_eq!(MobKind::Villager.max_health(), 20.0);
        // Vanilla files villagers under `MobCategory.MISC`, not `CREATURE`.
        assert_eq!(MobKind::Villager.props().category, "misc");
    }

    #[test]
    fn damage_respects_the_hurt_cooldown_and_knocks_back() {
        let (mut store, key) = spawn(MobKind::Cow);
        let from = Vec3::new(-1.0, 64.0, 0.5);
        assert_eq!(
            hurt(&mut store, key, 1.0, Some(from)),
            Some(Hurt::Survived { health: 9.0 })
        );
        // The mob is shoved east, away from the attacker.
        assert!(store.vel(key).unwrap().x > 0.3);
        assert_eq!(hurt(&mut store, key, 1.0, Some(from)), Some(Hurt::Immune));
        for _ in 0..HURT_COOLDOWN {
            tick_clock(&mut store, key);
        }
        assert_eq!(
            hurt(&mut store, key, 2.0, None),
            Some(Hurt::Survived { health: 7.0 })
        );
    }

    #[test]
    fn a_burning_mob_takes_one_damage_a_second_and_stops_after_its_time() {
        let mut rng = Rng::new(1);
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(MobKind::Cow, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        ignite(&mut store, key, 100);
        for _ in 0..100 {
            tick_clock(&mut store, key);
        }
        let state = state(&store, key).unwrap();
        assert_eq!(state.fire, 0);
        assert_eq!(state.health, MobKind::Cow.max_health() - 5.0);
    }

    #[test]
    fn lethal_damage_removes_the_mob_and_reports_death() {
        let (mut store, key) = spawn(MobKind::Sheep);
        assert_eq!(
            hurt(&mut store, key, MobKind::Sheep.max_health(), None),
            Some(Hurt::Died)
        );
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(state(&store, key).unwrap().health, 0.0);
        assert!(!is_mob(&store, key));
        assert_eq!(hurt(&mut store, key, 1.0, None), None);
        // `/kill` skips the cooldown.
        let (mut store, key) = spawn(MobKind::Pig);
        assert!(kill(&mut store, key));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert!(!kill(&mut store, key));
    }

    #[test]
    fn fall_damage_starts_above_the_safe_distance() {
        let (mut store, key) = spawn(MobKind::Cow);
        assert_eq!(apply_fall_damage(&mut store, key, 3.9), None);
        assert_eq!(
            apply_fall_damage(&mut store, key, 5.5),
            Some(Hurt::Survived { health: 8.0 })
        );
        let (mut store, key) = spawn(MobKind::Cow);
        assert_eq!(apply_fall_damage(&mut store, key, 40.0), Some(Hurt::Died));
    }

    #[test]
    fn passive_mobs_persist_until_their_despawn_policy_allows_it() {
        let (mut store, key) = spawn(MobKind::Cow);
        let mut rng = Rng::new(3);
        // Far from every player; animals still persist.
        assert!(!check_despawn(
            &mut store,
            key,
            &[Vec3::new(1000.0, 64.0, 1000.0)],
            false,
            &mut rng
        ));
        // A despawnable kind is removed beyond 128 blocks.
        state_mut(&mut store, key).unwrap().despawns = true;
        assert!(check_despawn(
            &mut store,
            key,
            &[Vec3::new(1000.0, 64.0, 1000.0)],
            false,
            &mut rng
        ));
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));

        // Inside 32 blocks the no-action timer resets and nothing despawns.
        let (mut store, key) = spawn(MobKind::Pig);
        state_mut(&mut store, key).unwrap().despawns = true;
        state_mut(&mut store, key).unwrap().no_action_time = NO_ACTION_TICKS + 1;
        assert!(!check_despawn(
            &mut store,
            key,
            &[Vec3::new(8.0, 64.0, 0.5)],
            false,
            &mut rng
        ));
        assert_eq!(state(&store, key).unwrap().no_action_time, 0);
    }

    #[test]
    fn clocks_advance_and_state_survives_a_region_hand_off() {
        let (mut store, key) = spawn(MobKind::Chicken);
        state_mut(&mut store, key).unwrap().health = 2.0;
        hurt(&mut store, key, 1.0, None);
        tick_clock(&mut store, key);
        assert_eq!(state(&store, key).unwrap().hurt_time, HURT_COOLDOWN - 1);
        assert_eq!(store.data(key).unwrap().tick_count, 1);
        // Walk into another region section: the record carries the living state.
        let pos = store.pos(key).unwrap();
        store.set_pos(key, Vec3::new(pos.x + 400.0, pos.y, pos.z));
        let mut out = store.take_crossing();
        assert_eq!(out.len(), 1);
        let mut other = EntityStore::default();
        other.insert(out.pop().unwrap());
        let key = other.keys()[0];
        let mob = state(&other, key).unwrap();
        assert_eq!(mob.health, 1.0);
        assert_eq!(mob.kind, MobKind::Chicken);
        assert!(other.check_invariants().is_ok());
    }
}
