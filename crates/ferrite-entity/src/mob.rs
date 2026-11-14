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
/// How long a provoked enderman hunts its attacker (`NeutralMob` persistent anger, ~30 s).
pub const ENDERMAN_ANGER_TICKS: u16 = 600;
/// Ticks a passive mob flees after being hurt (vanilla panic goal duration is 100-200).
pub const PANIC_TICKS: u16 = 100;
/// Vanilla `Chicken` waits 6000 to 11999 ticks between eggs.
pub const CHICKEN_EGG_MIN: u32 = 6000;
pub const CHICKEN_EGG_SPAN: i32 = 6000;

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
    /// `minecraft:trader_llama`: a llama led by a wandering trader.
    TraderLlama,
    /// `minecraft:wandering_trader`.
    WanderingTrader,
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
    pub const ALL_MOBS: [Self; 20] = [
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
        Self::TraderLlama,
        Self::WanderingTrader,
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
            Self::TraderLlama => "minecraft:trader_llama",
            Self::WanderingTrader => "minecraft:wandering_trader",
        }
    }

    /// Whether an owner can tame, seat and call this kind (`TamableAnimal`).
    pub fn is_tameable(self) -> bool {
        matches!(self, Self::Wolf | Self::Cat | Self::Parrot)
    }

    /// Horses, donkeys and mules: rideable, tamed by riding and saddled (`AbstractHorse`).
    pub fn is_equine(self) -> bool {
        matches!(
            self,
            Self::Horse | Self::Donkey | Self::Mule | Self::Llama | Self::TraderLlama
        )
    }

    /// A llama or a trader llama.
    pub fn is_llama(self) -> bool {
        matches!(self, Self::Llama | Self::TraderLlama)
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
            Self::TraderLlama => "minecraft:entities/trader_llama",
            Self::WanderingTrader => "minecraft:entities/wandering_trader",
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
            Self::TraderLlama => "Trader Llama",
            Self::WanderingTrader => "Wandering Trader",
        }
    }

    /// Vanilla `Animal.removeWhenFarAway` is false; monsters despawn when far away.
    pub fn despawns_when_far(self) -> bool {
        self.is_hostile()
    }
}

/// Vanilla `EquipmentSlot` values used by `set_equipment`.
pub const MAIN_HAND: u8 = 0;
pub const EQUIPMENT_SLOTS: usize = 6;

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
    /// The ender dragon's landing-cycle phase; unused by other mobs.
    pub dragon_phase: crate::dragon::DragonPhase,
    /// Ticks spent in [`Self::dragon_phase`].
    pub dragon_phase_ticks: u32,
    /// Flaming phases the perched dragon has finished (`DragonSittingFlamingPhase.flameCount`).
    pub perch_flames: u8,
    /// Where a charging dragon is headed, fixed when the charge started
    /// (`DragonChargePlayerPhase.targetLocation`). Not saved: a charge without a target aborts to
    /// the holding pattern, as vanilla does.
    pub dragon_charge_target: Option<ferrite_math::Vec3>,
    /// Ticks since the charge reached its target (`timeSinceCharge`); zero while still flying.
    pub dragon_charge_recovery: u32,
    /// The scanning dragon's yaw rate (`EnderDragon.yRotA`).
    pub dragon_yaw_rate: f32,
    /// A sheep's wool colour, the `DyeColor` id (`Sheep.DATA_WOOL_ID`).
    pub wool_color: u8,
    /// Whether a sheep has been sheared and not yet eaten grass.
    pub sheared: bool,
    /// Ticks left in a sheep's `EatBlockGoal`; zero when it is not eating.
    pub eat_timer: u8,
    /// Whether a zombie may break doors (`Zombie.canBreakDoors`).
    pub can_break_doors: bool,
    /// The door a zombie is chewing on, and for how many ticks (`BreakDoorGoal.breakTime`).
    pub door_pos: Option<[i32; 3]>,
    pub door_ticks: u16,
    /// The shaded spot a burning skeleton is running to (`FleeSunGoal.wantedX/Y/Z`).
    pub shade_pos: Option<[i32; 3]>,
    /// Whether a spider carries a skeleton jockey (`hasControllingPassenger`), which stops its
    /// leap. Set by the region that mounts the jockey.
    pub has_rider: bool,
    /// Whether a creeper is charged by lightning (`Creeper.DATA_IS_POWERED`).
    pub powered: bool,
    /// The creeper's synched swell direction: `1` while the fuse burns, `-1` otherwise.
    pub swell_dir: i8,
    /// Ticks an enderman has been stared at by `stare_target` (`aggroTime`); zero when calm.
    pub stare_time: u8,
    pub stare_target: Option<uuid::Uuid>,
    /// Whether anything ever stared at this enderman (`DATA_STARED_AT`).
    pub stared_at: bool,
    /// The block state an enderman carries (`DATA_CARRY_STATE`); kept and saved, not synched.
    pub carried: Option<u16>,
    /// Yaw the wander goal walks along, in degrees.
    pub wander_yaw: f32,
    /// Whether the wander goal wants to move during this interval.
    pub moving: bool,
    /// Player this mob is hunting, for hostile kinds (vanilla `Mob.target`).
    pub target: Option<uuid::Uuid>,
    /// The last player to damage this mob, used to credit the kill after its death tick.
    pub last_killer: Option<uuid::Uuid>,
    /// Ticks until the next melee attack (`Mob.attackStrengthTicker`-like cooldown).
    pub attack_cooldown: u16,
    /// Ticks of panic left after being hurt (vanilla `PanicGoal`).
    pub panic: u16,
    /// Where the panic flee goal runs from.
    pub panic_from: Option<Vec3>,
    /// Creeper swell counter (`Creeper.swell`).
    pub swell: u16,
    /// Held and worn item ids in vanilla `EquipmentSlot` order: main hand, off hand, feet,
    /// legs, chest, head. Zero means empty. Skeleton bows are the only default equipment.
    pub equipment: [u16; EQUIPMENT_SLOTS],
    /// Ticks until the skeleton's next arrow.
    pub bow_timer: u16,
    /// Ticks until a chicken lays its next egg (`Chicken.eggTime`); zero for other kinds.
    pub egg_time: u32,
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
    /// Index into [`crate::variant`]'s table for a cat or wolf; `None` until one is picked.
    pub variant: Option<u8>,
    /// Ticks spent next to the mate.
    pub breed_timer: u16,
    /// The player a tamed animal belongs to (`TamableAnimal.owner`).
    pub owner: Option<uuid::Uuid>,
    /// The player holding this animal on a lead, and that player's entity id for the client.
    pub leash: Option<(uuid::Uuid, i32)>,
    /// The llama this one trails in a caravan (`Llama.caravanHead`).
    pub caravan: Option<uuid::Uuid>,
    /// Ticks until the mob vanishes without loot (a wandering trader and its llamas); 0 for never.
    pub lifetime: u32,
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
            dragon_phase: crate::dragon::DragonPhase::Holding,
            dragon_phase_ticks: 0,
            perch_flames: 0,
            dragon_charge_target: None,
            dragon_charge_recovery: 0,
            dragon_yaw_rate: 0.0,
            wool_color: 0,
            sheared: false,
            eat_timer: 0,
            can_break_doors: false,
            door_pos: None,
            door_ticks: 0,
            shade_pos: None,
            has_rider: false,
            powered: false,
            swell_dir: -1,
            stare_time: 0,
            stare_target: None,
            stared_at: false,
            carried: None,
            wander_yaw: 0.0,
            moving: false,
            target: None,
            last_killer: None,
            attack_cooldown: 0,
            panic: 0,
            panic_from: None,
            swell: 0,
            equipment: if kind == MobKind::Skeleton {
                let mut equipment = [0; EQUIPMENT_SLOTS];
                equipment[MAIN_HAND as usize] = ferrite_item::item_id("minecraft:bow").unwrap_or(0);
                equipment
            } else {
                [0; EQUIPMENT_SLOTS]
            },
            bow_timer: 0,
            egg_time: 0,
            path: [[0; 3]; crate::path::MAX_PATH],
            path_len: 0,
            path_index: 0,
            path_timer: 0,
            age: 0,
            love: 0,
            love_cause: None,
            variant: None,
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
            lifetime: 0,
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
    if kind == MobKind::Chicken
        && let Some(mob) = data.mob.as_mut()
    {
        mob.egg_time = CHICKEN_EGG_MIN + rng.next_int(CHICKEN_EGG_SPAN) as u32;
    }
    if kind.is_animal()
        && let Some(state) = data.mob.as_ref()
    {
        crate::breed::define_metadata(&mut data.metadata, state);
    }
    if kind == MobKind::Enderman {
        crate::enderman::define_metadata(&mut data.metadata);
    }
    if kind == MobKind::Creeper {
        crate::creeper::define_metadata(&mut data.metadata);
    }
    if kind == MobKind::Spider {
        crate::spider::define_metadata(&mut data.metadata);
    }
    if kind == MobKind::Zombie
        && let Some(state) = data.mob.as_mut()
    {
        state.can_break_doors = rng.next_int(100) < crate::sun::CAN_BREAK_DOORS_CHANCE;
    }
    if kind == MobKind::Sheep
        && let Some(state) = data.mob.as_mut()
    {
        // `Sheep.finalizeSpawn` rolls the colour.
        state.wool_color = crate::sheep::random_color(rng);
        crate::sheep::define_metadata(&mut data.metadata, state);
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
        if kind.is_llama() {
            // `Llama.finalizeSpawn`: mostly strength 1 to 3, with a 4% chance of 1 to 5, and one
            // of four coats.
            data.metadata.set(
                crate::horse::STRENGTH_INDEX,
                ferrite_protocol::packets::play::MetaValue::Int(crate::horse::random_strength(rng)),
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
    if kind == MobKind::WanderingTrader {
        let mut state = crate::villager::VillagerState::new(
            crate::villager::VillagerType::Plains,
            [pos.x, pos.y, pos.z],
        );
        state.offers = crate::villager::trader_offers(data.uuid);
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
    hurt_with(store, key, amount, source, false)
}

/// [`hurt`] with control over armour. Sources tagged as bypassing armour (explosions in this
/// server's damage model) pass `bypass_armor` so the hit lands at full strength.
pub fn hurt_with(
    store: &mut EntityStore,
    key: EntityKey,
    amount: f32,
    source: Option<Vec3>,
    bypass_armor: bool,
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
    let amount = if state.armor_points > 0.0 && !bypass_armor {
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

/// Applies the health side of effect events to a mob (`LivingEntity` instant health/damage,
/// regeneration, poison and wither). Absorption, hunger and saturation do not apply to mobs
/// here. Poison never kills: it stops at half a heart.
fn apply_effect_events(
    store: &mut EntityStore,
    key: EntityKey,
    events: Vec<crate::effect::EffectEvent>,
) {
    use crate::effect::EffectEvent;
    for event in events {
        match event {
            EffectEvent::Heal(amount) => {
                if let Some(mob) = state_mut(store, key)
                    && !mob.is_dead()
                {
                    mob.health = (mob.health + amount).min(mob.kind.max_health());
                }
            }
            EffectEvent::Damage {
                amount, can_kill, ..
            } => {
                let Some(health) = state(store, key).map(|mob| mob.health) else {
                    continue;
                };
                let amount = if can_kill {
                    amount
                } else {
                    amount.min(health - 1.0)
                };
                hurt_with(store, key, amount, None, true);
            }
            _ => {}
        }
    }
}

/// `LivingEntity.addEffect` on a mob: stores the effect (a weaker instance never replaces a
/// stronger one) and applies instant health or damage at once. Returns the instance now
/// stored when the client must be told about it, `None` for instant effects, a kept stronger
/// instance or anything that is not a live mob.
pub fn add_effect(
    store: &mut EntityStore,
    key: EntityKey,
    instance: crate::effect::MobEffectInstance,
) -> Option<crate::effect::MobEffectInstance> {
    if !is_mob(store, key) {
        return None;
    }
    let kind = instance.kind();
    let events = store.data_mut(key)?.effects.add(instance);
    apply_effect_events(store, key, events);
    if kind.is_instant() {
        return None;
    }
    let current = *store.data(key)?.effects.get(kind)?;
    (current == instance).then_some(current)
}

/// An instant effect (healing or harming) at `scale` of its strength, as a splash potion or a
/// lingering cloud applies it (`applyInstantenousEffect`). Nothing is stored or announced.
pub fn apply_instant_scaled(
    store: &mut EntityStore,
    key: EntityKey,
    instance: crate::effect::MobEffectInstance,
    scale: f64,
) {
    if is_mob(store, key) {
        let events = crate::effect::ActiveEffects::instant_events(instance, scale);
        apply_effect_events(store, key, events);
    }
}

/// One tick of a mob's effects: applies regeneration, poison and wither and returns the
/// effects that expired, whose removal the client must be told about.
pub fn tick_effects(
    store: &mut EntityStore,
    key: EntityKey,
) -> Vec<crate::effect::MobEffectInstance> {
    if !is_mob(store, key) {
        return Vec::new();
    }
    let Some(data) = store.data_mut(key) else {
        return Vec::new();
    };
    if data.effects.is_empty() {
        return Vec::new();
    }
    let ticks = data.effects.tick();
    apply_effect_events(store, key, ticks.events);
    ticks.expired
}

/// Makes a hostile mob hunt `player` after being attacked (vanilla `Mob.setTarget` in
/// `LivingEntity.hurt`'s retaliation hook).
pub fn retaliate(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid) -> bool {
    let Some(mob) = state_mut(store, key) else {
        return false;
    };
    let kind_is_wolf = mob.kind == MobKind::Wolf;
    if matches!(
        mob.kind,
        MobKind::Wolf | MobKind::Llama | MobKind::TraderLlama
    ) {
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
    if mob.kind == MobKind::Enderman {
        mob.anger = ENDERMAN_ANGER_TICKS;
    }
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

/// Records `player` as the latest attacker, so a kill that happens on this or a later tick can be
/// credited by the region's death handling.
pub fn credit(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid) {
    if let Some(mob) = state_mut(store, key) {
        mob.last_killer = Some(player);
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
    fn a_killer_survives_the_mob_death_tick() {
        let (mut store, key) = spawn(MobKind::Cow);
        let player = uuid::Uuid::from_u128(0xc0ffee);
        credit(&mut store, key, player);
        assert_eq!(hurt(&mut store, key, 100.0, None), Some(Hurt::Died));
        assert_eq!(state(&store, key).unwrap().last_killer, Some(player));
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

    #[test]
    fn effects_on_a_mob_apply_instants_store_timed_ones_and_expire() {
        use crate::effect::{Effect, MobEffectInstance};
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(8.0, 64.0, 8.0),
            &mut Rng::new(1),
        ));
        state_mut(&mut store, key).unwrap().health = 2.0;
        // Instant health heals at once and is not stored or announced.
        let heal = MobEffectInstance::new(Effect::InstantHealth.id(), 1, 0);
        assert_eq!(add_effect(&mut store, key, heal), None);
        assert_eq!(state(&store, key).unwrap().health, 6.0);
        assert!(store.data(key).unwrap().effects.is_empty());
        // Instant damage hurts through armour-free magic damage.
        let harm = MobEffectInstance::new(Effect::InstantDamage.id(), 1, 0);
        assert_eq!(add_effect(&mut store, key, harm), None);
        assert_eq!(state(&store, key).unwrap().health, 0.0);
        assert!(!is_mob(&store, key));
    }

    #[test]
    fn a_timed_effect_is_stored_a_weaker_one_is_kept_out_and_it_expires() {
        use crate::effect::{Effect, MobEffectInstance};
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(8.0, 64.0, 8.0),
            &mut Rng::new(1),
        ));
        let strong = MobEffectInstance::new(Effect::Speed.id(), 3, 1);
        assert_eq!(add_effect(&mut store, key, strong), Some(strong));
        let weak = MobEffectInstance::new(Effect::Speed.id(), 100, 0);
        assert_eq!(add_effect(&mut store, key, weak), None);
        assert_eq!(store.data(key).unwrap().effects.level(Effect::Speed), 2);
        assert!(tick_effects(&mut store, key).is_empty());
        assert!(tick_effects(&mut store, key).is_empty());
        let expired = tick_effects(&mut store, key);
        assert_eq!(
            expired,
            vec![MobEffectInstance {
                duration: 0,
                ..strong
            }]
        );
        assert!(store.data(key).unwrap().effects.is_empty());
    }

    #[test]
    fn poison_stops_at_half_a_heart_and_regeneration_stops_at_full_health() {
        use crate::effect::{Effect, MobEffectInstance};
        let mut store = EntityStore::default();
        let key = store.insert(new_mob(
            MobKind::Cow,
            Vec3::new(8.0, 64.0, 8.0),
            &mut Rng::new(1),
        ));
        state_mut(&mut store, key).unwrap().health = 1.0;
        add_effect(
            &mut store,
            key,
            MobEffectInstance::new(Effect::Poison.id(), 25, 0),
        );
        for _ in 0..25 {
            tick_effects(&mut store, key);
        }
        assert_eq!(state(&store, key).unwrap().health, 1.0);
        add_effect(
            &mut store,
            key,
            MobEffectInstance::new(Effect::Regeneration.id(), 50, 0),
        );
        for _ in 0..50 {
            tick_effects(&mut store, key);
        }
        assert_eq!(state(&store, key).unwrap().health, 2.0);
    }
}
