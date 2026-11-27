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
/// How long an angered piglin hunts its attacker (`PiglinAi.ANGER_DURATION`, 600 ticks).
pub const PIGLIN_ANGER_TICKS: u16 = 600;
/// Baby piglin starting age (`AgeableMob.BABY_START_AGE`).
pub const PIGLIN_BABY_AGE: i32 = -24_000;
/// Vanilla `Blaze.DATA_FLAGS_ID` and `onFire` bit.
pub const BLAZE_FLAGS_INDEX: u8 = 16;
pub const BLAZE_FLAG_ON_FIRE: i8 = 1;
/// Vanilla `Witch.DATA_USING_ITEM` metadata index and drink duration (`Consumable.DEFAULT_CONSUME_SECONDS` * 20).
pub const WITCH_USING_ITEM_INDEX: u8 = 17;
pub const WITCH_DRINK_TICKS: u16 = 32;
/// Vanilla `Witch.getDamageAfterMagicAbsorb`: 85% resistance to `#minecraft:witch_resistant_to`.
pub const WITCH_MAGIC_DAMAGE_SCALE: f32 = 0.15;
/// Vanilla `Piglin.DATA_BABY_ID` and `Piglin.DATA_IS_CHARGING_CROSSBOW`.
pub const PIGLIN_BABY_INDEX: u8 = 17;
pub const PIGLIN_CHARGING_CROSSBOW_INDEX: u8 = 18;
/// Ticks a passive mob flees after being hurt (vanilla panic goal duration is 100-200).
pub const PANIC_TICKS: u16 = 100;
/// Vanilla `Chicken` waits 6000 to 11999 ticks between eggs.
pub const CHICKEN_EGG_MIN: u32 = 6000;
pub const CHICKEN_EGG_SPAN: i32 = 6000;

/// The mobs this server knows how to simulate: passive animals, neutral and hostile monsters,
/// villagers, golems, and bosses.
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
    Drowned,
    Husk,
    ZombieVillager,
    Stray,
    Bogged,
    WitherSkeleton,
    Blaze,
    CaveSpider,
    Witch,
    Piglin,
    ZombifiedPiglin,
    IronGolem,
    SnowGolem,
    Goat,
    Fox,
    Rabbit,
    Frog,
    Turtle,
    Bee,
    Mooshroom,
    Slime,
    MagmaCube,
    Hoglin,
    Zoglin,
    PiglinBrute,
    Vindicator,
    Pillager,
    Evoker,
    Ravager,
    Guardian,
    ElderGuardian,
    Phantom,
    Wither,
    Warden,
    Breeze,
    Creaking,
    Shulker,
    Strider,
    Allay,
    Axolotl,
    Camel,
    Sniffer,
    Armadillo,
    Ghast,
    Vex,
    Silverfish,
    Endermite,
    Squid,
    GlowSquid,
    Dolphin,
    Cod,
    Salmon,
    Pufferfish,
    TropicalFish,
    Tadpole,
    Bat,
    Ocelot,
    Panda,
    PolarBear,
    SkeletonHorse,
    ZombieHorse,
    Illusioner,
    Giant,
    CamelHusk,
    CopperGolem,
    HappyGhast,
    Nautilus,
    ZombieNautilus,
    Parched,
    SulfurCube,
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
    pub const ALL_MOBS: [Self; 90] = [
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
        Self::Drowned,
        Self::Husk,
        Self::ZombieVillager,
        Self::Stray,
        Self::Bogged,
        Self::WitherSkeleton,
        Self::Blaze,
        Self::CaveSpider,
        Self::Witch,
        Self::Piglin,
        Self::ZombifiedPiglin,
        Self::IronGolem,
        Self::SnowGolem,
        Self::Goat,
        Self::Fox,
        Self::Rabbit,
        Self::Frog,
        Self::Turtle,
        Self::Bee,
        Self::Mooshroom,
        Self::Slime,
        Self::MagmaCube,
        Self::Hoglin,
        Self::Zoglin,
        Self::PiglinBrute,
        Self::Vindicator,
        Self::Pillager,
        Self::Evoker,
        Self::Ravager,
        Self::Guardian,
        Self::ElderGuardian,
        Self::Phantom,
        Self::Wither,
        Self::Warden,
        Self::Breeze,
        Self::Creaking,
        Self::Shulker,
        Self::Strider,
        Self::Allay,
        Self::Axolotl,
        Self::Camel,
        Self::Sniffer,
        Self::Armadillo,
        Self::Ghast,
        Self::Vex,
        Self::Silverfish,
        Self::Endermite,
        Self::Squid,
        Self::GlowSquid,
        Self::Dolphin,
        Self::Cod,
        Self::Salmon,
        Self::Pufferfish,
        Self::TropicalFish,
        Self::Tadpole,
        Self::Bat,
        Self::Ocelot,
        Self::Panda,
        Self::PolarBear,
        Self::SkeletonHorse,
        Self::ZombieHorse,
        Self::Illusioner,
        Self::Giant,
        Self::CamelHusk,
        Self::CopperGolem,
        Self::HappyGhast,
        Self::Nautilus,
        Self::ZombieNautilus,
        Self::Parched,
        Self::SulfurCube,
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
            Self::Drowned => "minecraft:drowned",
            Self::Husk => "minecraft:husk",
            Self::ZombieVillager => "minecraft:zombie_villager",
            Self::Stray => "minecraft:stray",
            Self::Bogged => "minecraft:bogged",
            Self::WitherSkeleton => "minecraft:wither_skeleton",
            Self::Blaze => "minecraft:blaze",
            Self::CaveSpider => "minecraft:cave_spider",
            Self::Witch => "minecraft:witch",
            Self::Piglin => "minecraft:piglin",
            Self::ZombifiedPiglin => "minecraft:zombified_piglin",
            Self::IronGolem => "minecraft:iron_golem",
            Self::SnowGolem => "minecraft:snow_golem",
            Self::Goat => "minecraft:goat",
            Self::Fox => "minecraft:fox",
            Self::Rabbit => "minecraft:rabbit",
            Self::Frog => "minecraft:frog",
            Self::Turtle => "minecraft:turtle",
            Self::Bee => "minecraft:bee",
            Self::Mooshroom => "minecraft:mooshroom",
            Self::Slime => "minecraft:slime",
            Self::MagmaCube => "minecraft:magma_cube",
            Self::Hoglin => "minecraft:hoglin",
            Self::Zoglin => "minecraft:zoglin",
            Self::PiglinBrute => "minecraft:piglin_brute",
            Self::Vindicator => "minecraft:vindicator",
            Self::Pillager => "minecraft:pillager",
            Self::Evoker => "minecraft:evoker",
            Self::Ravager => "minecraft:ravager",
            Self::Guardian => "minecraft:guardian",
            Self::ElderGuardian => "minecraft:elder_guardian",
            Self::Phantom => "minecraft:phantom",
            Self::Wither => "minecraft:wither",
            Self::Warden => "minecraft:warden",
            Self::Breeze => "minecraft:breeze",
            Self::Creaking => "minecraft:creaking",
            Self::Shulker => "minecraft:shulker",
            Self::Strider => "minecraft:strider",
            Self::Allay => "minecraft:allay",
            Self::Axolotl => "minecraft:axolotl",
            Self::Camel => "minecraft:camel",
            Self::Sniffer => "minecraft:sniffer",
            Self::Armadillo => "minecraft:armadillo",
            Self::Ghast => "minecraft:ghast",
            Self::Vex => "minecraft:vex",
            Self::Silverfish => "minecraft:silverfish",
            Self::Endermite => "minecraft:endermite",
            Self::Squid => "minecraft:squid",
            Self::GlowSquid => "minecraft:glow_squid",
            Self::Dolphin => "minecraft:dolphin",
            Self::Cod => "minecraft:cod",
            Self::Salmon => "minecraft:salmon",
            Self::Pufferfish => "minecraft:pufferfish",
            Self::TropicalFish => "minecraft:tropical_fish",
            Self::Tadpole => "minecraft:tadpole",
            Self::Bat => "minecraft:bat",
            Self::Ocelot => "minecraft:ocelot",
            Self::Panda => "minecraft:panda",
            Self::PolarBear => "minecraft:polar_bear",
            Self::SkeletonHorse => "minecraft:skeleton_horse",
            Self::ZombieHorse => "minecraft:zombie_horse",
            Self::Illusioner => "minecraft:illusioner",
            Self::Giant => "minecraft:giant",
            Self::CamelHusk => "minecraft:camel_husk",
            Self::CopperGolem => "minecraft:copper_golem",
            Self::HappyGhast => "minecraft:happy_ghast",
            Self::Nautilus => "minecraft:nautilus",
            Self::ZombieNautilus => "minecraft:zombie_nautilus",
            Self::Parched => "minecraft:parched",
            Self::SulfurCube => "minecraft:sulfur_cube",
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
            Self::Horse
                | Self::Donkey
                | Self::Mule
                | Self::Llama
                | Self::TraderLlama
                | Self::SkeletonHorse
                | Self::ZombieHorse
        )
    }

    /// A llama or a trader llama.
    pub fn is_llama(self) -> bool {
        matches!(self, Self::Llama | Self::TraderLlama)
    }

    /// Zombie family (`Zombie`, `Drowned`, `Husk`, `ZombieVillager`, `ZombifiedPiglin`).
    pub fn is_zombie_like(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Drowned
                | Self::Husk
                | Self::ZombieVillager
                | Self::ZombifiedPiglin
        )
    }

    /// Skeleton family (`Skeleton`, `Stray`, `Bogged`, `WitherSkeleton`, `Parched`).
    pub fn is_skeleton_like(self) -> bool {
        matches!(
            self,
            Self::Skeleton | Self::Stray | Self::Bogged | Self::WitherSkeleton | Self::Parched
        )
    }

    /// Ranged bow skeleton (`Skeleton`, `Stray`, `Bogged`, `Parched`).
    pub fn is_bow_skeleton(self) -> bool {
        matches!(
            self,
            Self::Skeleton | Self::Stray | Self::Bogged | Self::Parched
        )
    }

    /// Spider family (`Spider`, `CaveSpider`).
    pub fn is_spider_like(self) -> bool {
        matches!(self, Self::Spider | Self::CaveSpider)
    }

    /// Vanilla `#minecraft:undead` entity tag: takes bonus damage from Smite.
    pub fn is_undead(self) -> bool {
        self.is_zombie_like()
            || self.is_skeleton_like()
            || matches!(
                self,
                Self::Phantom
                    | Self::Wither
                    | Self::SkeletonHorse
                    | Self::ZombieHorse
                    | Self::Zoglin
                    | Self::ZombieNautilus
                    | Self::CamelHusk
            )
    }

    /// Vanilla `#minecraft:arthropod` entity tag: takes bonus damage + Slowness IV from Bane of Arthropods.
    pub fn is_arthropod(self) -> bool {
        self.is_spider_like() || matches!(self, Self::Silverfish | Self::Endermite | Self::Bee)
    }

    /// Vanilla `#minecraft:sensitive_to_impaling` / aquatic mobs: takes bonus damage from Impaling.
    pub fn is_aquatic(self) -> bool {
        matches!(
            self,
            Self::Squid
                | Self::GlowSquid
                | Self::Dolphin
                | Self::Cod
                | Self::Salmon
                | Self::Pufferfish
                | Self::TropicalFish
                | Self::Tadpole
                | Self::Guardian
                | Self::ElderGuardian
                | Self::Turtle
                | Self::Axolotl
                | Self::Nautilus
                | Self::ZombieNautilus
        )
    }

    /// Whether open daylight ignites this monster when unhelmeted (`Mob.isSunBurnTick`).
    /// Husks, wither skeletons, and zombified piglins are immune to sunlight.
    pub fn burns_in_daylight(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Drowned
                | Self::ZombieVillager
                | Self::Skeleton
                | Self::Stray
                | Self::Bogged
                | Self::Phantom
                | Self::ZombieNautilus
        )
    }

    /// Whether this entity is immune to fire damage and ignition (`EntityType.fireImmune`).
    pub fn is_fire_immune(self) -> bool {
        matches!(
            self,
            Self::Blaze
                | Self::Ghast
                | Self::ZombifiedPiglin
                | Self::WitherSkeleton
                | Self::MagmaCube
                | Self::SulfurCube
                | Self::Zoglin
                | Self::Wither
                | Self::Strider
        )
    }

    /// Illagers and raiders (`Witch`, `Vindicator`, `Pillager`, `Evoker`, `Illusioner`, `Ravager`).
    pub fn is_raider(self) -> bool {
        matches!(
            self,
            Self::Witch
                | Self::Vindicator
                | Self::Pillager
                | Self::Evoker
                | Self::Illusioner
                | Self::Ravager
        )
    }

    /// Whether this kind hunts players (vanilla `Monster`/`Enemy`).
    pub fn is_hostile(self) -> bool {
        matches!(
            self,
            Self::Zombie
                | Self::Drowned
                | Self::Husk
                | Self::ZombieVillager
                | Self::Skeleton
                | Self::Stray
                | Self::Bogged
                | Self::WitherSkeleton
                | Self::Spider
                | Self::CaveSpider
                | Self::Creeper
                | Self::Enderman
                | Self::Blaze
                | Self::Witch
                | Self::Piglin
                | Self::ZombifiedPiglin
                | Self::Slime
                | Self::MagmaCube
                | Self::Hoglin
                | Self::Zoglin
                | Self::PiglinBrute
                | Self::Vindicator
                | Self::Pillager
                | Self::Evoker
                | Self::Illusioner
                | Self::Ravager
                | Self::Guardian
                | Self::ElderGuardian
                | Self::Phantom
                | Self::Wither
                | Self::Warden
                | Self::Breeze
                | Self::Creaking
                | Self::Shulker
                | Self::Ghast
                | Self::Vex
                | Self::Silverfish
                | Self::Endermite
                | Self::Giant
                | Self::Parched
                | Self::SulfurCube
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
            Self::Giant => 3.5,
            Self::Zombie
            | Self::Drowned
            | Self::Husk
            | Self::ZombieVillager
            | Self::ZombifiedPiglin
            | Self::WitherSkeleton
            | Self::Spider
            | Self::CaveSpider
            | Self::Enderman
            | Self::IronGolem
            | Self::Ravager
            | Self::Warden
            | Self::PolarBear => 2.5,
            Self::Hoglin | Self::Zoglin | Self::PiglinBrute | Self::Vindicator | Self::Creaking => {
                2.2
            }
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
            Self::Drowned => "minecraft:entities/drowned",
            Self::Husk => "minecraft:entities/husk",
            Self::ZombieVillager => "minecraft:entities/zombie_villager",
            Self::Stray => "minecraft:entities/stray",
            Self::Bogged => "minecraft:entities/bogged",
            Self::WitherSkeleton => "minecraft:entities/wither_skeleton",
            Self::Blaze => "minecraft:entities/blaze",
            Self::CaveSpider => "minecraft:entities/cave_spider",
            Self::Witch => "minecraft:entities/witch",
            Self::Piglin => "minecraft:entities/piglin",
            Self::ZombifiedPiglin => "minecraft:entities/zombified_piglin",
            Self::IronGolem => "minecraft:entities/iron_golem",
            Self::SnowGolem => "minecraft:entities/snow_golem",
            Self::Goat => "minecraft:entities/goat",
            Self::Fox => "minecraft:entities/fox",
            Self::Rabbit => "minecraft:entities/rabbit",
            Self::Frog => "minecraft:entities/frog",
            Self::Turtle => "minecraft:entities/turtle",
            Self::Bee => "minecraft:entities/bee",
            Self::Mooshroom => "minecraft:entities/mooshroom",
            Self::Slime => "minecraft:entities/slime",
            Self::MagmaCube => "minecraft:entities/magma_cube",
            Self::Hoglin => "minecraft:entities/hoglin",
            Self::Zoglin => "minecraft:entities/zoglin",
            Self::PiglinBrute => "minecraft:entities/piglin_brute",
            Self::Vindicator => "minecraft:entities/vindicator",
            Self::Pillager => "minecraft:entities/pillager",
            Self::Evoker => "minecraft:entities/evoker",
            Self::Ravager => "minecraft:entities/ravager",
            Self::Guardian => "minecraft:entities/guardian",
            Self::ElderGuardian => "minecraft:entities/elder_guardian",
            Self::Phantom => "minecraft:entities/phantom",
            Self::Wither => "minecraft:entities/wither",
            Self::Warden => "minecraft:entities/warden",
            Self::Breeze => "minecraft:entities/breeze",
            Self::Creaking => "minecraft:entities/creaking",
            Self::Shulker => "minecraft:entities/shulker",
            Self::Strider => "minecraft:entities/strider",
            Self::Allay => "minecraft:entities/allay",
            Self::Axolotl => "minecraft:entities/axolotl",
            Self::Camel => "minecraft:entities/camel",
            Self::Sniffer => "minecraft:entities/sniffer",
            Self::Armadillo => "minecraft:entities/armadillo",
            Self::Ghast => "minecraft:entities/ghast",
            Self::Vex => "minecraft:entities/vex",
            Self::Silverfish => "minecraft:entities/silverfish",
            Self::Endermite => "minecraft:entities/endermite",
            Self::Squid => "minecraft:entities/squid",
            Self::GlowSquid => "minecraft:entities/glow_squid",
            Self::Dolphin => "minecraft:entities/dolphin",
            Self::Cod => "minecraft:entities/cod",
            Self::Salmon => "minecraft:entities/salmon",
            Self::Pufferfish => "minecraft:entities/pufferfish",
            Self::TropicalFish => "minecraft:entities/tropical_fish",
            Self::Tadpole => "minecraft:entities/tadpole",
            Self::Bat => "minecraft:entities/bat",
            Self::Ocelot => "minecraft:entities/ocelot",
            Self::Panda => "minecraft:entities/panda",
            Self::PolarBear => "minecraft:entities/polar_bear",
            Self::SkeletonHorse => "minecraft:entities/skeleton_horse",
            Self::ZombieHorse => "minecraft:entities/zombie_horse",
            Self::Illusioner => "minecraft:entities/illusioner",
            Self::Giant => "minecraft:entities/giant",
            Self::CamelHusk => "minecraft:entities/camel_husk",
            Self::CopperGolem => "minecraft:entities/copper_golem",
            Self::HappyGhast => "minecraft:entities/happy_ghast",
            Self::Nautilus => "minecraft:entities/nautilus",
            Self::ZombieNautilus => "minecraft:entities/zombie_nautilus",
            Self::Parched => "minecraft:entities/parched",
            Self::SulfurCube => "minecraft:entities/sulfur_cube",
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
            Self::Drowned => "Drowned",
            Self::Husk => "Husk",
            Self::ZombieVillager => "Zombie Villager",
            Self::Stray => "Stray",
            Self::Bogged => "Bogged",
            Self::WitherSkeleton => "Wither Skeleton",
            Self::Blaze => "Blaze",
            Self::CaveSpider => "Cave Spider",
            Self::Witch => "Witch",
            Self::Piglin => "Piglin",
            Self::ZombifiedPiglin => "Zombified Piglin",
            Self::IronGolem => "Iron Golem",
            Self::SnowGolem => "Snow Golem",
            Self::Goat => "Goat",
            Self::Fox => "Fox",
            Self::Rabbit => "Rabbit",
            Self::Frog => "Frog",
            Self::Turtle => "Turtle",
            Self::Bee => "Bee",
            Self::Mooshroom => "Mooshroom",
            Self::Slime => "Slime",
            Self::MagmaCube => "Magma Cube",
            Self::Hoglin => "Hoglin",
            Self::Zoglin => "Zoglin",
            Self::PiglinBrute => "Piglin Brute",
            Self::Vindicator => "Vindicator",
            Self::Pillager => "Pillager",
            Self::Evoker => "Evoker",
            Self::Ravager => "Ravager",
            Self::Guardian => "Guardian",
            Self::ElderGuardian => "Elder Guardian",
            Self::Phantom => "Phantom",
            Self::Wither => "Wither",
            Self::Warden => "Warden",
            Self::Breeze => "Breeze",
            Self::Creaking => "Creaking",
            Self::Shulker => "Shulker",
            Self::Strider => "Strider",
            Self::Allay => "Allay",
            Self::Axolotl => "Axolotl",
            Self::Camel => "Camel",
            Self::Sniffer => "Sniffer",
            Self::Armadillo => "Armadillo",
            Self::Ghast => "Ghast",
            Self::Vex => "Vex",
            Self::Silverfish => "Silverfish",
            Self::Endermite => "Endermite",
            Self::Squid => "Squid",
            Self::GlowSquid => "Glow Squid",
            Self::Dolphin => "Dolphin",
            Self::Cod => "Cod",
            Self::Salmon => "Salmon",
            Self::Pufferfish => "Pufferfish",
            Self::TropicalFish => "Tropical Fish",
            Self::Tadpole => "Tadpole",
            Self::Bat => "Bat",
            Self::Ocelot => "Ocelot",
            Self::Panda => "Panda",
            Self::PolarBear => "Polar Bear",
            Self::SkeletonHorse => "Skeleton Horse",
            Self::ZombieHorse => "Zombie Horse",
            Self::Illusioner => "Illusioner",
            Self::Giant => "Giant",
            Self::CamelHusk => "Camel Husk",
            Self::CopperGolem => "Copper Golem",
            Self::HappyGhast => "Happy Ghast",
            Self::Nautilus => "Nautilus",
            Self::ZombieNautilus => "Zombie Nautilus",
            Self::Parched => "Parched",
            Self::SulfurCube => "Sulfur Cube",
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

/// Potions brewed, drunk, or thrown by witches (`Witch.aiStep` / `Witch.performRangedAttack`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitchPotion {
    WaterBreathing,
    FireResistance,
    Healing,
    Swiftness,
    Slowness,
    Poison,
    Weakness,
    Harming,
}

impl WitchPotion {
    /// Vanilla `minecraft:potion` registry short name.
    pub const fn potion_name(self) -> &'static str {
        match self {
            Self::WaterBreathing => "water_breathing",
            Self::FireResistance => "fire_resistance",
            Self::Healing => "healing",
            Self::Swiftness => "swiftness",
            Self::Slowness => "slowness",
            Self::Poison => "poison",
            Self::Weakness => "weakness",
            Self::Harming => "harming",
        }
    }

    /// Creates a drinkable (`"minecraft:potion"`) or splash (`"minecraft:splash_potion"`) stack.
    pub fn stack(self, item_name: &str) -> ferrite_item::ItemStack {
        let mut stack = ferrite_item::ItemStack::from_name(item_name)
            .unwrap_or_else(|| ferrite_item::ItemStack::with_count(0, 1));
        stack.components.insert(
            ferrite_item::component_ids::POTION_CONTENTS,
            ferrite_item::Component::PotionContents(ferrite_item::PotionContents {
                potion: Some(format!("minecraft:{}", self.potion_name())),
                ..Default::default()
            }),
        );
        stack
    }
}

/// Vanilla `CrossbowAttack.CrossbowState` for piglins wielding crossbows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CrossbowState {
    #[default]
    Uncharged,
    Charging,
    Charged,
    ReadyToAttack,
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
    /// The ender dragon's landing-cycle phase; unused by other mobs.
    pub dragon_phase: crate::dragon::DragonPhase,
    /// Ticks spent in [`Self::dragon_phase`].
    pub dragon_phase_ticks: u32,
    /// Flaming phases the perched dragon has finished (`DragonSittingFlamingPhase.flameCount`).
    pub perch_flames: u8,
    /// Where a charging dragon is headed, fixed when the charge started
    /// (`DragonChargePlayerPhase.targetLocation`). Saved as `DragonChargeTarget` so a restart
    /// resumes the charge; a save without a target still aborts to the holding pattern, as
    /// vanilla does.
    pub dragon_charge_target: Option<ferrite_math::Vec3>,
    /// Ticks since the charge reached its target (`timeSinceCharge`); zero while still flying.
    pub dragon_charge_recovery: u32,
    /// The scanning dragon's yaw rate (`EnderDragon.yRotA`).
    pub dragon_yaw_rate: f32,
    /// The last [`crate::dragon::FLIGHT_HISTORY`] ticks of the dragon's height and yaw
    /// (`DragonFlightHistory`), used to place the head, neck and tail parts exactly as
    /// `EnderDragon.aiStep` does.
    pub dragon_flight: crate::dragon::DragonFlight,
    /// A sheep's wool colour, the `DyeColor` id (`Sheep.DATA_WOOL_ID`).
    pub wool_color: u8,
    /// Whether a sheep has been sheared and not yet eaten grass.
    pub sheared: bool,
    /// Whether an iron golem was built by a player (`IronGolem.DATA_FLAGS_ID` bit `0x01`).
    pub player_created: bool,
    /// Ticks left in a sheep's `EatBlockGoal`; zero when it is not eating.
    pub eat_timer: u8,
    /// Whether a zombie may break doors (`Zombie.canBreakDoors`).
    pub can_break_doors: bool,
    /// The door a zombie is chewing on, and for how many ticks (`BreakDoorGoal.breakTime`).
    pub door_pos: Option<[i32; 3]>,
    pub door_ticks: u16,
    /// Lower half of the hand-openable door this mob opened and may close behind itself
    /// (`MemoryModuleType.DOORS_TO_CLOSE` / `InteractWithDoor`).
    pub opened_door: Option<[i32; 3]>,
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
    /// The block state an enderman carries (`DATA_CARRY_STATE`), synced at metadata index 16.
    pub carried: Option<u16>,
    /// The turtle's home beach (`Turtle.homePos`), saved as `home_pos`.
    pub home: Option<[i32; 3]>,
    /// The bell a zombie is walking through (`MoveThroughVillageGoal`); transient.
    pub village_target: Option<[i32; 3]>,
    /// Ticks until the next village scan; transient.
    pub village_timer: u16,
    /// A turtle carries a clutch and must return home to lay it (`Turtle.HAS_EGG`).
    pub has_egg: bool,
    /// A turtle is standing on its nest counting down (`Turtle.LAYING_EGG`).
    pub laying_egg: bool,
    /// Ticks spent laying; vanilla does not save this transient counter.
    pub lay_egg_counter: u16,
    /// The sand block a turtle has chosen for its nest.
    pub lay_egg_target: Option<[i32; 3]>,
    /// A turtle is walking home (`Turtle.goingHome`). The rare idle trip and the egg trip share
    /// this state; it is transient, like vanilla's goal flag.
    pub going_home: bool,
    /// Ticks spent within 16 blocks of home on this trip
    /// (`TurtleGoHomeGoal.closeToHomeTryTicks`, capped at 600).
    pub home_trip_ticks: u16,
    /// The water block a turtle is walking to (`TurtleGoToWaterGoal.blockPos`).
    pub water_target: Option<[i32; 3]>,
    /// Ticks spent trying to reach `water_target` (`MoveToBlockGoal.tryTicks`, max 1200).
    pub water_try_ticks: u16,
    /// A swimming turtle's far target (`Turtle.travelPos`); transient.
    pub travel_pos: Option<[i32; 3]>,
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
    /// Whether a creeper was ignited by flint and steel (`Creeper.DATA_IS_IGNITED`).
    pub ignited: bool,
    /// Ticks a zombie has spent submerged in water (`Zombie.inWaterTime`).
    pub in_water_time: u16,
    /// Whether a chicken spawned as a baby-zombie jockey (`Chicken.isChickenJockey`).
    pub is_chicken_jockey: bool,
    /// Whether a bee has already stung a target (`Bee.FLAG_HAS_STUNG`).
    pub has_stung: bool,
    /// Ticks elapsed since a bee stung (`Bee.timeSinceSting`).
    pub sting_ticks: u16,
    /// Whether a bee carries nectar (`Bee.FLAG_HAS_NECTAR` / `HasNectar`).
    pub has_nectar: bool,
    /// The hive block a bee calls home (`Bee.hivePos`, saved as `hive_pos`).
    pub hive_pos: Option<[i32; 3]>,
    /// The flower block a bee last pollinated (`Bee.savedFlowerPos`, saved as `flower_pos`).
    pub flower_pos: Option<[i32; 3]>,
    /// Crops grown since the last nectar drop-off (`Bee.numCropsGrownSincePollination`, max 10).
    pub crops_grown: u8,
    /// Ticks since the bee last pollinated (`Bee.ticksSincePollination`).
    pub ticks_since_pollination: u16,
    /// Ticks a bee must stay out of hives (`Bee.stayOutOfHiveCountdown`, saved as
    /// `CannotEnterHiveTicks`).
    pub stay_out_of_hive: u16,
    /// Ticks a bee has searched for nectar (`Bee.ticksWithoutNectarSinceExitingHive`).
    pub ticks_without_nectar: u16,
    /// Per-kind cooldown for special goals (goat ram, rabbit hop delay, fox pounce, frog tongue).
    pub special_cooldown: u16,
    /// Active phase tick counter for multi-tick special goals (goat ram, fox crouch, frog tongue).
    pub special_timer: u16,
    /// Cooldown between long jumps (goat and frog `LONG_JUMP_COOLING_DOWN`).
    pub long_jump_cooldown: u16,
    /// Target position locked by a goat's ram preparation/charge.
    pub ram_target: Option<Vec3>,
    /// Multi-phase state byte for special goals (0 = idle; goat 1=prepare/2=charge; fox 1=crouch/2=pounce; frog 1=tongue/2=catch).
    pub ram_phase: u8,
    /// Blaze charged/on-fire synched flag (`Blaze.DATA_FLAGS_ID` bit `0x01`).
    pub charged: bool,
    /// Blaze volley step (`0` idle, `1` charging 60 ticks, `2..=4` firing 3 fireballs).
    pub attack_step: u8,
    /// Consecutive ticks a blaze lacked line of sight to its target (`BlazeAttackGoal.lastSeen`).
    pub blaze_last_seen: u16,
    /// Blaze vertical hover offset above the player (`Blaze.allowedHeightOffset`).
    pub height_offset: f32,
    /// Ticks until a blaze rolls a new [`Self::height_offset`].
    pub height_offset_timer: u16,
    /// Witch item-drinking synched flag (`Witch.DATA_USING_ITEM`).
    pub using_item: bool,
    /// Witch raid-victory celebration flag (`Raider.IS_CELEBRATING`, metadata index 16).
    pub celebrating: bool,
    /// This raider leads a raid wave; killing it gives the player Bad Omen. Transient, like a
    /// patrol leader's banner.
    pub raid_captain: bool,
    /// Remaining ticks while a witch finishes drinking its held potion.
    pub drink_timer: u16,
    /// Potion currently being drunk by a witch (applied on completion without faking inventory).
    pub drink_potion: Option<WitchPotion>,
    /// Consecutive ticks a witch has had line of sight (`RangedAttackGoal.seeTime`).
    pub see_time: u16,
    /// Whether the witch last suffered fire damage (`Witch.aiStep` fire-resistance check).
    pub last_damage_fire: bool,
    /// Piglin crossbow state (`CrossbowAttack.CrossbowState`).
    pub crossbow_state: CrossbowState,
    /// Piglin charging synched flag (`Piglin.DATA_IS_CHARGING_CROSSBOW`, metadata index 18).
    pub charging_crossbow: bool,
    /// Remaining ticks a piglin admires a held gold ingot (`MemoryModuleType.ADMIRING_ITEM`).
    pub admiring: u16,
    /// Goat screaming variant (`Goat.DATA_IS_SCREAMING_GOAT`).
    pub screaming: bool,
    /// Whether a goat still carries each horn (`Goat.DATA_HAS_LEFT_HORN` /
    /// `DATA_HAS_RIGHT_HORN`). Both default to true; a unihorn spawn clears one.
    pub has_left_horn: bool,
    pub has_right_horn: bool,
    /// Fox trusted players (`Fox.DATA_TRUSTED_ID_0`/`_1`), at most two.
    pub trusted: [Option<uuid::Uuid>; 2],
    /// Fox crouching synched flag; [`Self::sitting`] carries the sitting bit.
    pub fox_crouching: bool,
    /// Fox sleeping synched flag (`Fox.FLAG_SLEEPING`).
    pub fox_sleeping: bool,
    /// Piglin dancing (`Piglin.DATA_IS_DANCING`), set by the unmodelled hunt celebration.
    pub dancing: bool,
    /// Selected 26.3 sound-variant registry id; `None` keeps the kind's vanilla default.
    pub sound_variant: Option<u8>,
    /// Cube size for `Slime` and `MagmaCube` (`AbstractCubeMob.ID_SIZE`, clamped `1..=127`).
    pub cube_size: u8,
    /// Ticks spent by a piglin outside a `piglin_safe` dimension (`AbstractPiglin.timeInOverworld`).
    pub time_in_overworld: u16,
    /// Whether this piglin is immune to Overworld zombification (`AbstractPiglin.DATA_IMMUNE_TO_ZOMBIFICATION`).
    pub immune_to_zombification: bool,
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
            dragon_flight: crate::dragon::DragonFlight::default(),
            wool_color: 0,
            sheared: false,
            player_created: false,
            eat_timer: 0,
            can_break_doors: false,
            door_pos: None,
            door_ticks: 0,
            opened_door: None,
            shade_pos: None,
            has_rider: false,
            powered: false,
            swell_dir: -1,
            ignited: false,
            in_water_time: 0,
            is_chicken_jockey: false,
            has_stung: false,
            sting_ticks: 0,
            has_nectar: false,
            hive_pos: None,
            flower_pos: None,
            crops_grown: 0,
            ticks_since_pollination: 0,
            stay_out_of_hive: 0,
            ticks_without_nectar: 0,
            special_cooldown: 0,
            special_timer: 0,
            long_jump_cooldown: 0,
            ram_target: None,
            ram_phase: 0,
            stare_time: 0,
            stare_target: None,
            stared_at: false,
            carried: None,
            home: None,
            village_target: None,
            village_timer: 0,
            has_egg: false,
            laying_egg: false,
            lay_egg_counter: 0,
            lay_egg_target: None,
            going_home: false,
            home_trip_ticks: 0,
            water_target: None,
            water_try_ticks: 0,
            travel_pos: None,
            wander_yaw: 0.0,
            moving: false,
            target: None,
            last_killer: None,
            attack_cooldown: 0,
            panic: 0,
            panic_from: None,
            swell: 0,
            cube_size: 1,
            equipment: if kind.is_bow_skeleton() {
                let mut equipment = [0; EQUIPMENT_SLOTS];
                equipment[MAIN_HAND as usize] = ferrite_item::item_id("minecraft:bow").unwrap_or(0);
                equipment
            } else if kind == MobKind::WitherSkeleton {
                let mut equipment = [0; EQUIPMENT_SLOTS];
                equipment[MAIN_HAND as usize] =
                    ferrite_item::item_id("minecraft:stone_sword").unwrap_or(0);
                equipment
            } else if matches!(kind, MobKind::Piglin | MobKind::ZombifiedPiglin) {
                let mut equipment = [0; EQUIPMENT_SLOTS];
                equipment[MAIN_HAND as usize] =
                    ferrite_item::item_id("minecraft:golden_sword").unwrap_or(0);
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
            charged: false,
            attack_step: 0,
            blaze_last_seen: 0,
            height_offset: 0.5,
            height_offset_timer: 0,
            using_item: false,
            celebrating: false,
            raid_captain: false,
            drink_timer: 0,
            drink_potion: None,
            see_time: 0,
            last_damage_fire: false,
            crossbow_state: CrossbowState::Uncharged,
            charging_crossbow: false,
            admiring: 0,
            screaming: false,
            has_left_horn: true,
            has_right_horn: true,
            trusted: [None; 2],
            fox_crouching: false,
            fox_sleeping: false,
            dancing: false,
            sound_variant: None,
            time_in_overworld: 0,
            immune_to_zombification: false,
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
    new_mob_with_difficulty(kind, pos, rng, true)
}

/// A new mob at `pos` using `hard` to gate Hard-only `Zombie.finalizeSpawn` door-breaking rolls.
pub fn new_mob_with_difficulty(
    kind: MobKind,
    pos: Vec3,
    rng: &mut Rng,
    hard: bool,
) -> EntityRecord {
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
    data.metadata.define_mob(kind);
    if kind == MobKind::Turtle
        && let Some(mob) = data.mob.as_mut()
    {
        // `Turtle.finalizeSpawn`: the spawn block is the home position.
        mob.home = Some([
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        ]);
    }
    if kind == MobKind::Chicken
        && let Some(mob) = data.mob.as_mut()
    {
        mob.egg_time = CHICKEN_EGG_MIN + rng.next_int(CHICKEN_EGG_SPAN) as u32;
    }
    if hard
        && matches!(
            kind,
            MobKind::Zombie | MobKind::Husk | MobKind::Drowned | MobKind::ZombieVillager
        )
        && let Some(mob) = data.mob.as_mut()
    {
        mob.can_break_doors = rng.next_int(100) < crate::sun::CAN_BREAK_DOORS_CHANCE;
    }
    if kind == MobKind::ZombieVillager {
        data.villager = Some(crate::villager::VillagerState::new(
            crate::villager::VillagerType::Plains,
            [pos.x, pos.y, pos.z],
        ));
    }
    if kind == MobKind::Sheep
        && let Some(mob) = data.mob.as_mut()
    {
        // `Sheep.finalizeSpawn` rolls the colour.
        mob.wool_color = crate::sheep::random_color(rng);
    }
    if kind == MobKind::Goat
        && let Some(mob) = data.mob.as_mut()
    {
        // `Goat.finalizeSpawn`: 2% screamers; a further 10% of adults spawn with one horn.
        mob.screaming = rng.next_f32() < crate::goat::SCREAMING_CHANCE;
        if rng.next_f32() < crate::goat::UNIHORN_CHANCE {
            if rng.next_int(2) == 0 {
                mob.has_left_horn = false;
            } else {
                mob.has_right_horn = false;
            }
        }
        // `GoatAi.initMemories`: screaming goats ram far more often.
        mob.special_cooldown = crate::goat::ram_cooldown(mob.screaming, rng);
        mob.long_jump_cooldown = 600 + rng.next_int(600) as u16;
    }
    if matches!(kind, MobKind::Slime | MobKind::MagmaCube)
        && let Some(mob) = data.mob.as_mut()
    {
        // `Slime.finalizeSpawn`: natural sizes 1, 2, or 4 (`1 << nextInt(3)`).
        let size = 1u8 << (rng.next_int(3) as u8);
        set_cube_size_state(mob, size);
    }
    if kind == MobKind::Frog
        && let Some(mob) = data.mob.as_mut()
    {
        mob.long_jump_cooldown = 100 + rng.next_int(40) as u16;
    }
    if kind.is_equine() {
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
    if kind == MobKind::Piglin {
        data.piglin_inventory = Some(crate::piglin::PiglinInventory::new());
        let is_baby = rng.next_f32() < 0.2;
        if is_baby {
            if let Some(mob) = data.mob.as_mut() {
                mob.age = PIGLIN_BABY_AGE;
                mob.equipment[MAIN_HAND as usize] = 0;
            }
        } else {
            // `Piglin.finalizeSpawn`: 50% crossbow, else 10% golden_spear / 90% golden_sword,
            // plus 10% chance per golden armor slot.
            let weapon = if rng.next_f32() < 0.5 {
                "minecraft:crossbow"
            } else if rng.next_int(10) == 0 {
                "minecraft:golden_spear"
            } else {
                "minecraft:golden_sword"
            };
            if let Some(stack) = ferrite_item::ItemStack::from_name(weapon) {
                set_equipment_stack_on_data(&mut data, crate::EquipmentSlot::MainHand, Some(stack));
            }
            for (slot, item) in [
                (crate::EquipmentSlot::Head, "minecraft:golden_helmet"),
                (crate::EquipmentSlot::Chest, "minecraft:golden_chestplate"),
                (crate::EquipmentSlot::Legs, "minecraft:golden_leggings"),
                (crate::EquipmentSlot::Feet, "minecraft:golden_boots"),
            ] {
                if rng.next_f32() < 0.1
                    && let Some(stack) = ferrite_item::ItemStack::from_name(item)
                {
                    set_equipment_stack_on_data(&mut data, slot, Some(stack));
                }
            }
            data.equipment.clear_dirty();
        }
    }
    // `Cow`/`Chicken`/`Pig`/`Wolf`/`Cat.finalizeSpawn` pick a random sound variant.
    if crate::variant::sound_names(kind).is_some()
        && let Some(mob) = data.mob.as_mut()
    {
        mob.sound_variant = Some(crate::variant::random_sound_index(kind, rng));
    }
    sync_state_metadata(&mut data);
    data.metadata.clear_dirty();
    let mut flags = Flags::default();
    if kind == MobKind::EnderDragon {
        // Vanilla `EnderDragon.<init>`: `this.noPhysics = true`.
        flags.set(Flags::NO_PHYSICS, true);
    }
    EntityRecord {
        pos,
        vel: Vec3::ZERO,
        flags,
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

/// Sets one of the six equipment slots (`0..6`: main hand, off hand, feet, legs, chest, head)
/// on any living mob. Returns `false` if `key` is not a mob or `slot >= EQUIPMENT_SLOTS`.
pub fn set_equipment(store: &mut EntityStore, key: EntityKey, slot: usize, item_id: u16) -> bool {
    if slot >= EQUIPMENT_SLOTS {
        return false;
    }
    let Some(eq_slot) = crate::EquipmentSlot::from_index(slot as u8) else {
        return false;
    };
    let stack = (item_id != 0).then(|| ferrite_item::ItemStack::with_count(item_id, 1));
    set_equipment_stack(store, key, eq_slot, stack)
}

/// Vanilla `Zombie.DATA_BABY_ID`, `DATA_SPECIAL_TYPE_ID`, `DATA_DROWNED_CONVERSION_ID`.
pub const ZOMBIE_BABY_INDEX: u8 = 16;
pub const ZOMBIE_SPECIAL_TYPE_INDEX: u8 = 17;
pub const ZOMBIE_DROWNED_CONVERSION_INDEX: u8 = 18;
/// Vanilla `ZombieVillager.DATA_CONVERTING_ID` & `DATA_VILLAGER_DATA`.
pub const ZOMBIE_VILLAGER_CONVERTING_INDEX: u8 = 19;
pub const ZOMBIE_VILLAGER_DATA_INDEX: u8 = 20;
/// Vanilla `Skeleton.DATA_STRAY_CONVERSION_ID`.
pub const SKELETON_STRAY_CONVERSION_INDEX: u8 = 16;

/// Defines `Zombie` synched data values and writes the current state.
pub fn define_zombie_metadata(metadata: &mut crate::Metadata, state: &MobState) {
    use ferrite_protocol::packets::play::MetaValue;
    metadata.define(ZOMBIE_BABY_INDEX, MetaValue::Boolean(false));
    metadata.define(ZOMBIE_SPECIAL_TYPE_INDEX, MetaValue::Int(0));
    metadata.define(ZOMBIE_DROWNED_CONVERSION_INDEX, MetaValue::Boolean(false));
    sync_zombie_metadata(metadata, state);
}

/// Syncs `Zombie` baby and drowned-conversion flags from `MobState`.
pub fn sync_zombie_metadata(metadata: &mut crate::Metadata, state: &MobState) {
    use ferrite_protocol::packets::play::MetaValue;
    metadata.set(ZOMBIE_BABY_INDEX, MetaValue::Boolean(state.age < 0));
    metadata.set(
        ZOMBIE_DROWNED_CONVERSION_INDEX,
        MetaValue::Boolean(state.in_water_time >= crate::ai::DROWNED_CONVERSION_TICKS),
    );
}

/// Syncs `MobState`-backed per-type metadata into `data.metadata` before tracker emission.
pub fn sync_state_metadata(data: &mut EntityData) {
    use ferrite_protocol::packets::play::MetaValue;
    data.metadata.set_custom_name(data.custom_name.as_deref());
    let Some(state) = data.mob else {
        if let Some(arrow) = data.arrow {
            data.metadata.set(
                crate::metadata::arrow::FLAGS,
                MetaValue::Byte(if arrow.critical {
                    crate::metadata::arrow::FLAG_CRITICAL
                } else {
                    0
                }),
            );
        }
        return;
    };
    data.metadata.set(
        crate::metadata::living::HEALTH,
        MetaValue::Float(state.health),
    );
    data.metadata.set(
        crate::metadata::mob::MOB_FLAGS,
        MetaValue::Byte(if state.no_ai {
            crate::metadata::mob::FLAG_NO_AI as i8
        } else {
            0
        }),
    );
    if state.kind.is_ageable() {
        data.metadata
            .set(crate::breed::BABY_INDEX, MetaValue::Boolean(state.age < 0));
    }
    match state.kind {
        MobKind::ZombieVillager => {
            sync_zombie_metadata(&mut data.metadata, &state);
            data.metadata.set(
                ZOMBIE_VILLAGER_CONVERTING_INDEX,
                MetaValue::Boolean(state.special_timer > 0),
            );
            if let Some(villager) = data.villager.as_ref() {
                data.metadata.set(
                    ZOMBIE_VILLAGER_DATA_INDEX,
                    MetaValue::VillagerData {
                        kind: villager.villager_type.id(),
                        profession: villager.profession.id(),
                        level: i32::from(villager.level),
                    },
                );
            }
        }
        k if k.is_zombie_like() => sync_zombie_metadata(&mut data.metadata, &state),
        MobKind::EnderDragon => {
            crate::dragon::sync_metadata(&mut data.metadata, state.dragon_phase)
        }
        MobKind::Sheep => crate::sheep::write_metadata(&mut data.metadata, &state),
        MobKind::Creeper => {
            data.metadata.set(
                crate::creeper::SWELL_DIR_INDEX,
                MetaValue::Int(i32::from(state.swell_dir)),
            );
            data.metadata.set(
                crate::creeper::POWERED_INDEX,
                MetaValue::Boolean(state.powered),
            );
            data.metadata.set(
                crate::creeper::IGNITED_INDEX,
                MetaValue::Boolean(state.ignited),
            );
        }
        MobKind::Enderman => {
            data.metadata.set(
                crate::enderman::CARRIED_BLOCK_INDEX,
                MetaValue::OptionalBlockState(state.carried),
            );
            data.metadata.set(
                crate::enderman::STARED_AT_INDEX,
                MetaValue::Boolean(state.stared_at),
            );
        }
        MobKind::Villager => {
            if let Some(villager) = data.villager.as_ref() {
                crate::villager::sync_metadata(&mut data.metadata, villager);
            }
        }
        MobKind::Bogged => {
            data.metadata.set(
                crate::metadata::bogged::SHEARED,
                MetaValue::Boolean(state.sheared),
            );
        }
        MobKind::IronGolem => {
            data.metadata.set(
                crate::metadata::iron_golem::FLAGS,
                MetaValue::Byte(if state.player_created {
                    crate::metadata::iron_golem::FLAG_PLAYER_CREATED
                } else {
                    0
                }),
            );
        }
        MobKind::Wither => {
            data.metadata.set(
                crate::metadata::wither::INVULNERABLE_TIME,
                MetaValue::Int(i32::from(state.special_timer)),
            );
        }
        MobKind::SnowGolem => {
            data.metadata.set(
                crate::metadata::snow_golem::PUMPKIN,
                MetaValue::Byte(if state.sheared {
                    0
                } else {
                    crate::metadata::snow_golem::FLAG_HAS_PUMPKIN
                }),
            );
        }
        MobKind::Goat => {
            data.metadata.set(
                crate::metadata::goat::IS_SCREAMING,
                MetaValue::Boolean(state.screaming),
            );
            data.metadata.set(
                crate::metadata::goat::HAS_LEFT_HORN,
                MetaValue::Boolean(state.has_left_horn),
            );
            data.metadata.set(
                crate::metadata::goat::HAS_RIGHT_HORN,
                MetaValue::Boolean(state.has_right_horn),
            );
        }
        MobKind::Blaze => {
            data.metadata.set(
                BLAZE_FLAGS_INDEX,
                MetaValue::Byte(if state.charged { BLAZE_FLAG_ON_FIRE } else { 0 }),
            );
        }
        MobKind::Witch => {
            data.metadata
                .set(WITCH_USING_ITEM_INDEX, MetaValue::Boolean(state.using_item));
            data.metadata.set(
                crate::metadata::witch::CELEBRATING,
                MetaValue::Boolean(state.celebrating),
            );
        }
        MobKind::Piglin => {
            data.metadata.set(
                crate::metadata::piglin::IMMUNE_TO_ZOMBIFICATION,
                MetaValue::Boolean(state.immune_to_zombification),
            );
            data.metadata
                .set(PIGLIN_BABY_INDEX, MetaValue::Boolean(state.age < 0));
            data.metadata.set(
                PIGLIN_CHARGING_CROSSBOW_INDEX,
                MetaValue::Boolean(state.charging_crossbow),
            );
            data.metadata.set(
                crate::metadata::piglin::DANCING,
                MetaValue::Boolean(state.dancing),
            );
        }
        MobKind::Bee => {
            let anger = if state.anger > 0 {
                i64::from(state.anger)
            } else {
                -1
            };
            data.metadata
                .set(crate::metadata::bee::ANGER_END_TIME, MetaValue::Long(anger));
        }
        MobKind::Fox => {
            if let Some(v) = state.variant {
                data.metadata
                    .set(crate::metadata::fox::TYPE, MetaValue::Int(i32::from(v)));
            }
            let mut flags = 0;
            if state.sitting {
                flags |= crate::metadata::fox::FLAG_SITTING;
            }
            if state.fox_crouching {
                flags |= crate::metadata::fox::FLAG_CROUCHING;
            }
            if state.fox_sleeping {
                flags |= crate::metadata::fox::FLAG_SLEEPING;
            }
            data.metadata
                .set(crate::metadata::fox::FLAGS, MetaValue::Byte(flags as i8));
            data.metadata.set(
                crate::metadata::fox::TRUSTED_0,
                MetaValue::OptionalUuid(state.trusted[0]),
            );
            data.metadata.set(
                crate::metadata::fox::TRUSTED_1,
                MetaValue::OptionalUuid(state.trusted[1]),
            );
        }
        MobKind::Rabbit => {
            if let Some(v) = state.variant {
                data.metadata
                    .set(crate::metadata::rabbit::TYPE, MetaValue::Int(i32::from(v)));
            }
        }
        MobKind::Frog => {
            let v = state
                .variant
                .map_or(crate::metadata::frog::DEFAULT_VARIANT, i32::from);
            data.metadata
                .set(crate::metadata::frog::VARIANT, MetaValue::FrogVariant(v));
        }
        MobKind::Mooshroom => {
            if let Some(v) = state.variant {
                data.metadata.set(
                    crate::metadata::mooshroom::TYPE,
                    MetaValue::Int(i32::from(v)),
                );
            }
        }
        MobKind::Slime | MobKind::MagmaCube => {
            data.metadata.set(
                crate::metadata::cube_mob::SIZE,
                MetaValue::Int(i32::from(state.cube_size.clamp(1, 127))),
            );
        }
        MobKind::Cow => {
            let v = state.variant.map_or(
                crate::metadata::animal_variant::DEFAULT_TEMPERATE,
                i32::from,
            );
            data.metadata.set(
                crate::metadata::animal_variant::COW_VARIANT,
                MetaValue::CowVariant(v),
            );
        }
        MobKind::Pig => {
            let v = state.variant.map_or(
                crate::metadata::animal_variant::DEFAULT_TEMPERATE,
                i32::from,
            );
            data.metadata.set(
                crate::metadata::animal_variant::PIG_VARIANT,
                MetaValue::PigVariant(v),
            );
        }
        MobKind::Chicken => {
            let v = state.variant.map_or(
                crate::metadata::animal_variant::DEFAULT_TEMPERATE,
                i32::from,
            );
            data.metadata.set(
                crate::metadata::animal_variant::CHICKEN_VARIANT,
                MetaValue::ChickenVariant(v),
            );
        }
        MobKind::Turtle => crate::turtle::sync_metadata(&mut data.metadata, &state),
        MobKind::Hoglin => {
            data.metadata.set(
                crate::metadata::hoglin::IMMUNE_TO_ZOMBIFICATION,
                MetaValue::Boolean(state.immune_to_zombification),
            );
        }
        MobKind::Zoglin => {
            data.metadata.set(
                crate::metadata::zoglin::BABY,
                MetaValue::Boolean(state.age < 0),
            );
        }
        MobKind::PiglinBrute => {
            data.metadata.set(
                crate::metadata::piglin::IMMUNE_TO_ZOMBIFICATION,
                MetaValue::Boolean(state.immune_to_zombification),
            );
        }
        MobKind::Vindicator | MobKind::Ravager => {
            data.metadata.set(
                crate::metadata::raider::CELEBRATING,
                MetaValue::Boolean(state.celebrating),
            );
        }
        MobKind::Pillager => {
            data.metadata.set(
                crate::metadata::raider::CELEBRATING,
                MetaValue::Boolean(state.celebrating),
            );
            data.metadata.set(
                crate::metadata::raider::CHARGING_CROSSBOW,
                MetaValue::Boolean(state.charging_crossbow),
            );
        }
        MobKind::Evoker => {
            data.metadata.set(
                crate::metadata::raider::CELEBRATING,
                MetaValue::Boolean(state.celebrating),
            );
        }
        MobKind::Axolotl => {
            if let Some(v) = state.variant {
                data.metadata.set(
                    crate::metadata::axolotl::VARIANT,
                    MetaValue::Int(i32::from(v)),
                );
            }
        }
        k if k.is_tameable() => crate::tame::write_metadata(&mut data.metadata, &state),
        k if k.is_equine() => crate::horse::write_metadata(&mut data.metadata, &state),
        _ => {}
    }
    crate::variant::sync_sound_metadata(&mut data.metadata, state.kind, state.sound_variant);
}

/// Sets the cube size (`1..=127`) and scales health/speed on `state` (`AbstractCubeMob.setSize`).
pub fn set_cube_size_state(state: &mut MobState, size: u8) {
    let size = size.clamp(1, 127);
    state.cube_size = size;
    let s = f32::from(size);
    state.health = s * s;
    state.speed = 0.2 + 0.1 * s;
}

/// Sets a `Slime` or `MagmaCube` size and syncs its `ID_SIZE` metadata.
pub fn set_cube_size(store: &mut EntityStore, key: EntityKey, size: u8) {
    let Some(data) = store.data_mut(key) else {
        return;
    };
    if let Some(state) = data.mob.as_mut()
        && matches!(state.kind, MobKind::Slime | MobKind::MagmaCube)
    {
        set_cube_size_state(state, size);
    }
    sync_state_metadata(data);
}

/// Whether vanilla 26.3 models `slot` on `kind`.
///
/// The standard six slots (`MainHand`, `OffHand`, `Feet`, `Legs`, `Chest`, `Head`) are
/// available on every mob; `Body` is modelled on horses, llamas, trader llamas, and wolves;
/// `Saddle` is modelled on pigs, horses, donkeys, and mules.
pub const fn can_use_equipment_slot(kind: MobKind, slot: crate::EquipmentSlot) -> bool {
    use crate::EquipmentSlot;
    match slot {
        EquipmentSlot::MainHand
        | EquipmentSlot::OffHand
        | EquipmentSlot::Feet
        | EquipmentSlot::Legs
        | EquipmentSlot::Chest
        | EquipmentSlot::Head => true,
        EquipmentSlot::Body => matches!(
            kind,
            MobKind::Horse | MobKind::Llama | MobKind::TraderLlama | MobKind::Wolf
        ),
        EquipmentSlot::Saddle => {
            matches!(
                kind,
                MobKind::Pig
                    | MobKind::Strider
                    | MobKind::Horse
                    | MobKind::Donkey
                    | MobKind::Mule
                    | MobKind::Camel
            )
        }
    }
}

/// Sets one of the eight equipment slots on `data` directly (used by [`set_equipment_stack`]
/// and spawn initialization).
pub(crate) fn set_equipment_stack_on_data(
    data: &mut EntityData,
    slot: crate::EquipmentSlot,
    item: Option<ferrite_item::ItemStack>,
) -> bool {
    let Some(kind) = data.mob.as_ref().map(|mob| mob.kind) else {
        return false;
    };
    if !can_use_equipment_slot(kind, slot) {
        return false;
    }
    let normalized = item.filter(|stack| !stack.is_empty());
    let item_id = normalized.as_ref().map_or(0, |stack| stack.item);
    if let Some(mob) = data.mob.as_mut() {
        let idx = slot.as_index();
        if idx < EQUIPMENT_SLOTS {
            mob.equipment[idx] = item_id;
        } else if slot == crate::EquipmentSlot::Body {
            mob.armor = item_id;
            mob.armor_points = normalized
                .as_ref()
                .and_then(|s| s.name())
                .map_or(0.0, crate::horse::armor_points);
        } else if slot == crate::EquipmentSlot::Saddle {
            mob.saddled = normalized.is_some();
        }
    }
    data.equipment.set(slot, normalized);
    true
}

/// Sets one of the eight equipment slots on any living mob using a full [`ferrite_item::ItemStack`],
/// keeping `MobState::equipment` / `MobState::armor` / `MobState::saddled` and `EntityData::equipment`
/// in sync and marking the slot dirty. Returns `false` if `key` is not a mob or `slot` is not
/// modelled on its [`MobKind`].
pub fn set_equipment_stack(
    store: &mut EntityStore,
    key: EntityKey,
    slot: crate::EquipmentSlot,
    item: Option<ferrite_item::ItemStack>,
) -> bool {
    let Some(data) = store.data_mut(key) else {
        return false;
    };
    set_equipment_stack_on_data(data, slot, item)
}

/// Reads the full [`ferrite_item::ItemStack`] in `slot` for `key`, falling back to `MobState`
/// defaults when `EntityData::equipment` has no explicit stack.
pub fn equipment_stack(
    store: &EntityStore,
    key: EntityKey,
    slot: crate::EquipmentSlot,
) -> Option<ferrite_item::ItemStack> {
    let data = store.data(key)?;
    if let Some(stack) = data.equipment.get(slot) {
        return Some(stack.clone());
    }
    let state = data.mob.as_ref()?;
    if !can_use_equipment_slot(state.kind, slot) {
        return None;
    }
    let idx = slot.as_index();
    if idx < EQUIPMENT_SLOTS {
        let item = state.equipment[idx];
        (item != 0).then(|| ferrite_item::ItemStack::with_count(item, 1))
    } else if slot == crate::EquipmentSlot::Body && state.armor != 0 {
        Some(ferrite_item::ItemStack::with_count(state.armor, 1))
    } else if slot == crate::EquipmentSlot::Saddle && state.saddled {
        ferrite_item::ItemStack::from_name("minecraft:saddle")
    } else {
        None
    }
}

/// Reads the six-slot equipment array (`[main_hand, off_hand, feet, legs, chest, head]`) of `key`.
pub fn equipment(store: &EntityStore, key: EntityKey) -> Option<[u16; EQUIPMENT_SLOTS]> {
    state(store, key).map(|mob| mob.equipment)
}

/// Returns the drop chance of `slot` for `key` (defaults to [`crate::equipment::DEFAULT_DROP_CHANCE`]).
pub fn drop_chance(store: &EntityStore, key: EntityKey, slot: crate::EquipmentSlot) -> Option<f32> {
    let data = store.data(key)?;
    data.mob.as_ref()?;
    Some(data.equipment.drop_chance(slot))
}

/// Sets the drop chance of `slot` for `key` (finite, non-negative values only).
pub fn set_drop_chance(
    store: &mut EntityStore,
    key: EntityKey,
    slot: crate::EquipmentSlot,
    chance: f32,
) -> bool {
    if !chance.is_finite() || chance < 0.0 {
        return false;
    }
    let Some(data) = store.data_mut(key) else {
        return false;
    };
    if data.mob.is_none() {
        return false;
    }
    data.equipment.set_drop_chance(slot, chance);
    true
}

/// Marks `slot` on `key` as a guaranteed/preserved drop (`2.0f`).
pub fn set_guaranteed_drop(
    store: &mut EntityStore,
    key: EntityKey,
    slot: crate::EquipmentSlot,
) -> bool {
    set_drop_chance(store, key, slot, crate::equipment::GUARANTEED_DROP_CHANCE)
}

/// Returns all eight per-slot drop chances of `key` in wire order.
pub fn drop_chances(
    store: &EntityStore,
    key: EntityKey,
) -> Option<[f32; crate::equipment::SLOT_COUNT]> {
    let data = store.data(key)?;
    data.mob.as_ref()?;
    Some(data.equipment.drop_chances())
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
    if state.hurt_time > 0 || (state.kind == MobKind::Wither && state.special_timer > 0) {
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
    // Passive mobs panic and flee from the attacker (vanilla `PanicGoal`); monsters, wolves,
    // iron golems, and unstung bees retaliate instead of fleeing.
    if let Some(source) = source
        && let Some(mob) = state_mut(store, key)
        && !mob.kind.is_hostile()
        && !matches!(mob.kind, MobKind::Wolf | MobKind::IronGolem)
        && !(mob.kind == MobKind::Bee && !mob.has_stung)
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
                amount,
                can_kill,
                wither,
            } => {
                let Some((kind, health)) = state(store, key).map(|mob| (mob.kind, mob.health))
                else {
                    continue;
                };
                // Vanilla `Witch.getDamageAfterMagicAbsorb`: 85% resistance to magic / poison
                // (`DamageTypeTags.WITCH_RESISTANT_TO`), excluding wither.
                let amount = if kind == MobKind::Witch && !wither {
                    amount * WITCH_MAGIC_DAMAGE_SCALE
                } else {
                    amount
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
/// `LivingEntity.hurt`'s retaliation hook). A player-created iron golem refuses player
/// retaliation, matching `IronGolem.canAttack`.
pub fn retaliate(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid) -> bool {
    let Some(mob) = state_mut(store, key) else {
        return false;
    };
    let kind = mob.kind;
    if kind == MobKind::IronGolem && mob.player_created {
        return false;
    }
    if matches!(
        kind,
        MobKind::Wolf
            | MobKind::Llama
            | MobKind::TraderLlama
            | MobKind::Piglin
            | MobKind::ZombifiedPiglin
            | MobKind::IronGolem
            | MobKind::Bee
            | MobKind::PolarBear
    ) {
        // A wild wolf, llama, piglin, zombified piglin, iron golem, or unstung bee turns on
        // whoever hurt it; a tame animal or already-stung bee never does.
        if mob.owner.is_some() || (kind == MobKind::Bee && mob.has_stung) {
            return false;
        }
        let anger = if kind == MobKind::Piglin {
            PIGLIN_ANGER_TICKS
        } else {
            WOLF_ANGER_TICKS
        };
        mob.target = Some(player);
        mob.anger = anger;
        if kind == MobKind::Piglin {
            crate::piglin::cancel_admiring(store, key);
        }
        if matches!(
            kind,
            MobKind::Wolf | MobKind::Piglin | MobKind::ZombifiedPiglin | MobKind::Bee
        ) {
            alert_pack(store, key, player, kind);
        }
        return true;
    }
    if !kind.is_hostile() {
        return false;
    }
    mob.target = Some(player);
    if kind == MobKind::Enderman {
        mob.anger = ENDERMAN_ANGER_TICKS;
    }
    true
}

/// Wild wolves (and zombified piglins) within this distance join one that was attacked.
const PACK_ALERT_RADIUS: f64 = 16.0;

/// Every other wild mob of `kind` near `key` turns on `player` as well.
fn alert_pack(store: &mut EntityStore, key: EntityKey, player: uuid::Uuid, kind: MobKind) {
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
            }) && state(store, other).is_some_and(|m| {
                m.kind == kind && m.owner.is_none() && !(kind == MobKind::Bee && m.has_stung)
            })
        })
        .collect();
    let anger = if kind == MobKind::Piglin {
        PIGLIN_ANGER_TICKS
    } else {
        WOLF_ANGER_TICKS
    };
    for other in near {
        if let Some(mob) = state_mut(store, other) {
            mob.target = Some(player);
            mob.anger = anger;
        }
    }
}

/// Vanilla `AbstractPiglin.CONVERSION_TIME`: ticks outside a piglin-safe dimension before converting.
pub const PIGLIN_ZOMBIFICATION_TICKS: u16 = 300;
/// Vanilla `AbstractPiglin.finishConversion`: duration of Nausea I applied to the converted mob.
pub const PIGLIN_CONVERSION_NAUSEA_TICKS: i32 = 200;

/// Ticks Overworld zombification on a `Piglin`. Returns `true` when the timer exceeds
/// [`PIGLIN_ZOMBIFICATION_TICKS`] (`> 300`) and it is ready to convert to `ZombifiedPiglin`.
pub fn tick_piglin_zombification(
    store: &mut EntityStore,
    key: EntityKey,
    piglin_safe_dimension: bool,
) -> bool {
    let Some(mob) = state_mut(store, key) else {
        return false;
    };
    if mob.kind != MobKind::Piglin {
        return false;
    }
    if piglin_safe_dimension || mob.immune_to_zombification || mob.no_ai {
        mob.time_in_overworld = 0;
        return false;
    }
    mob.time_in_overworld = mob.time_in_overworld.saturating_add(1);
    mob.time_in_overworld > PIGLIN_ZOMBIFICATION_TICKS
}

/// Vanilla `Piglin.finishConversion`: converts a `Piglin` into a `ZombifiedPiglin` preserving
/// health, baby age, equipment, custom name, position, velocity, and orientation, replacing
/// piglin accessors with zombie accessors and applying 200 ticks of Nausea I.
pub fn convert_zombified_piglin(store: &mut EntityStore, key: EntityKey) -> Option<MobKind> {
    if state(store, key)?.kind != MobKind::Piglin {
        return None;
    }
    let data = store.data_mut(key)?;
    let props = MobKind::ZombifiedPiglin.props();
    data.kind = MobKind::ZombifiedPiglin.entity_type();
    data.width = f64::from(props.width);
    data.height = f64::from(props.height);
    data.piglin_inventory = None;
    if let Some(mob) = data.mob.as_mut() {
        mob.kind = MobKind::ZombifiedPiglin;
        mob.time_in_overworld = 0;
        mob.admiring = 0;
        mob.charging_crossbow = false;
        mob.crossbow_state = CrossbowState::Uncharged;
        mob.dancing = false;
        mob.health = mob.health.min(MobKind::ZombifiedPiglin.max_health());
    }
    data.effects.add(crate::effect::MobEffectInstance::new(
        crate::effect::Effect::Nausea.id(),
        PIGLIN_CONVERSION_NAUSEA_TICKS,
        0,
    ));
    data.metadata.clear_accessors_from(16);
    data.metadata.define_mob(MobKind::ZombifiedPiglin);
    sync_state_metadata(data);
    Some(MobKind::ZombifiedPiglin)
}

/// Vanilla `Zombie.convertToZombieType`: converts a submerged `Zombie` into a `Drowned` (or a
/// submerged `Husk` into a `Zombie`), preserving health, custom name, baby age, equipment,
/// position, velocity, and orientation.
pub fn convert_submerged_zombie(store: &mut EntityStore, key: EntityKey) -> Option<MobKind> {
    let target_kind = match state(store, key)?.kind {
        MobKind::Zombie => MobKind::Drowned,
        MobKind::Husk => MobKind::Zombie,
        _ => return None,
    };
    let data = store.data_mut(key)?;
    let props = target_kind.props();
    data.kind = target_kind.entity_type();
    data.width = f64::from(props.width);
    data.height = f64::from(props.height);
    if let Some(mob) = data.mob.as_mut() {
        mob.kind = target_kind;
        mob.in_water_time = 0;
        mob.health = mob.health.min(target_kind.max_health());
    }
    sync_state_metadata(data);
    Some(target_kind)
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
    knockback_with(store, key, source, 0.4, 0.0);
}

/// Knockback with configurable horizontal strength and extra vertical launch (used by
/// `IronGolem.doHurtTarget`'s +0.4 fling and `Goat`'s heavy ram knockback).
pub fn knockback_with(
    store: &mut EntityStore,
    key: EntityKey,
    source: Vec3,
    strength_xz: f64,
    extra_y: f64,
) {
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
    vel.x = dx / length * strength_xz;
    vel.z = dz / length * strength_xz;
    if store
        .flags(key)
        .is_some_and(|flags| flags.has(Flags::ON_GROUND))
    {
        vel.y = vel.y.max(0.4);
    }
    vel.y += extra_y;
    store.set_vel(key, vel);
    if let Some(flags) = store.flags_mut(key) {
        flags.set(Flags::NEEDS_SYNC, true);
    }
}

/// Vanilla `LivingEntity.checkFallDamage`, applied with the type's `safe_fall_distance` and
/// per-mob fall-damage immunity or reduction (`IronGolem`, `SnowGolem`, `Bee`, `Goat`, `Frog`).
pub fn apply_fall_damage(store: &mut EntityStore, key: EntityKey, distance: f32) -> Option<Hurt> {
    let kind = state(store, key)?.kind;
    if matches!(
        kind,
        MobKind::IronGolem
            | MobKind::SnowGolem
            | MobKind::Bee
            | MobKind::Chicken
            | MobKind::Parrot
            | MobKind::Blaze
    ) {
        return None;
    }
    let safe = entity_props::default_attribute(kind.entity_type(), "safe_fall_distance")
        .unwrap_or(3.0) as f32;
    let reduction = match kind {
        MobKind::Goat => 10.0,
        MobKind::Frog => 5.0,
        _ => 0.0,
    };
    let damage = (distance - safe - reduction).floor();
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
        if let Some(mob) = state_mut(store, key)
            && mob.kind == MobKind::Witch
        {
            mob.last_damage_fire = true;
        }
        hurt(store, key, 1.0, None);
    }
}

/// Sets a mob on fire for `ticks` (vanilla `Entity.igniteForSeconds`); it keeps the longer time.
pub fn ignite(store: &mut EntityStore, key: EntityKey, ticks: u16) {
    if let Some(mob) = state_mut(store, key) {
        if mob.kind.is_fire_immune() {
            return;
        }
        mob.fire = mob.fire.max(ticks);
        if mob.kind == MobKind::Witch {
            mob.last_damage_fire = true;
        }
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

    #[test]
    fn all_39_mob_kinds_resolve_and_zombie_conversion_and_piglin_pack_alert_work() {
        for kind in MobKind::ALL_MOBS {
            assert_eq!(MobKind::from_name(kind.name()), Some(kind));
            assert_eq!(MobKind::from_entity_type(kind.entity_type()), Some(kind));
            assert!(kind.max_health() > 0.0);
            let (store, key) = spawn(kind);
            assert!(is_mob(&store, key));
        }

        let (mut store, zkey) = spawn(MobKind::Zombie);
        state_mut(&mut store, zkey).unwrap().health = 12.0;
        assert_eq!(
            convert_submerged_zombie(&mut store, zkey),
            Some(MobKind::Drowned)
        );
        assert_eq!(state(&store, zkey).unwrap().kind, MobKind::Drowned);
        assert_eq!(state(&store, zkey).unwrap().health, 12.0);
        assert_eq!(
            store.data(zkey).unwrap().kind,
            MobKind::Drowned.entity_type()
        );

        let (mut store, hkey) = spawn(MobKind::Husk);
        assert_eq!(
            convert_submerged_zombie(&mut store, hkey),
            Some(MobKind::Zombie)
        );
        assert_eq!(state(&store, hkey).unwrap().kind, MobKind::Zombie);

        let mut store = EntityStore::default();
        let mut rng = Rng::new(5);
        let p1 = store.insert(new_mob(
            MobKind::ZombifiedPiglin,
            Vec3::new(0.0, 64.0, 0.0),
            &mut rng,
        ));
        let p2 = store.insert(new_mob(
            MobKind::ZombifiedPiglin,
            Vec3::new(5.0, 64.0, 0.0),
            &mut rng,
        ));
        let attacker = uuid::Uuid::from_u128(0xBEEF);
        assert!(retaliate(&mut store, p1, attacker));
        assert_eq!(state(&store, p1).unwrap().target, Some(attacker));
        assert_eq!(state(&store, p2).unwrap().target, Some(attacker));
        assert_eq!(state(&store, p2).unwrap().anger, WOLF_ANGER_TICKS);
    }

    #[test]
    fn piglin_spawn_tables_pack_alert_and_blaze_witch_resistances() {
        use ferrite_protocol::packets::play::MetaValue;

        let mut rng = Rng::new(77);
        let mut babies = 0;
        let mut crossbows = 0;
        let mut swords = 0;
        let mut spears = 0;
        let mut armored = 0;
        let crossbow_id = ferrite_item::item_id("minecraft:crossbow").unwrap();
        let sword_id = ferrite_item::item_id("minecraft:golden_sword").unwrap();
        let spear_id = ferrite_item::item_id("minecraft:golden_spear").unwrap();

        for _ in 0..100 {
            let rec = new_mob(MobKind::Piglin, Vec3::new(0.5, 64.0, 0.5), &mut rng);
            let s = rec.data.mob.unwrap();
            if s.age < 0 {
                babies += 1;
                assert_eq!(s.age, PIGLIN_BABY_AGE);
                assert_eq!(s.equipment[MAIN_HAND as usize], 0);
                assert_eq!(
                    rec.data.metadata.get(PIGLIN_BABY_INDEX),
                    Some(&MetaValue::Boolean(true))
                );
            } else {
                let main = s.equipment[MAIN_HAND as usize];
                if main == crossbow_id {
                    crossbows += 1;
                } else if main == sword_id {
                    swords += 1;
                } else if main == spear_id {
                    spears += 1;
                } else {
                    panic!("unexpected piglin mainhand item id {main}");
                }
                if s.equipment[2..].iter().any(|&id| id != 0) {
                    armored += 1;
                }
            }
        }
        assert!(babies > 5 && babies < 40, "babies={babies}");
        assert!(crossbows > 15 && swords > 10 && spears >= 1);
        assert!(armored > 5, "armored={armored}");

        // Piglin pack alert + 600-tick anger duration.
        let mut store = EntityStore::default();
        let p1 = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(0.5, 64.0, 0.5),
            &mut rng,
        ));
        let p2 = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(8.5, 64.0, 0.5),
            &mut rng,
        ));
        let far = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(25.5, 64.0, 0.5),
            &mut rng,
        ));
        let attacker = uuid::Uuid::from_u128(0xF161u128);
        assert!(retaliate(&mut store, p1, attacker));
        assert_eq!(state(&store, p1).unwrap().anger, PIGLIN_ANGER_TICKS);
        assert_eq!(state(&store, p2).unwrap().target, Some(attacker));
        assert_eq!(state(&store, p2).unwrap().anger, PIGLIN_ANGER_TICKS);
        assert_eq!(state(&store, far).unwrap().target, None);

        // Blaze immunity to fire ignition & fall damage + charged flag sync.
        let blaze = store.insert(new_mob(MobKind::Blaze, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        ignite(&mut store, blaze, 100);
        assert_eq!(state(&store, blaze).unwrap().fire, 0);
        assert!(apply_fall_damage(&mut store, blaze, 20.0).is_none());
        state_mut(&mut store, blaze).unwrap().charged = true;
        sync_state_metadata(store.data_mut(blaze).unwrap());
        assert_eq!(
            store.data(blaze).unwrap().metadata.get(BLAZE_FLAGS_INDEX),
            Some(&MetaValue::Byte(BLAZE_FLAG_ON_FIRE))
        );

        // Witch 85% resistance to InstantDamage / Poison + using_item sync.
        let witch = store.insert(new_mob(MobKind::Witch, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        let start_hp = state(&store, witch).unwrap().health;
        apply_instant_scaled(
            &mut store,
            witch,
            crate::effect::MobEffectInstance::new(crate::effect::Effect::InstantDamage.id(), 1, 0),
            1.0,
        );
        let dmg_taken = start_hp - state(&store, witch).unwrap().health;
        assert!(
            (dmg_taken - 6.0 * WITCH_MAGIC_DAMAGE_SCALE).abs() < 1e-4,
            "expected 85% magic reduction (0.9 dmg), got {dmg_taken}"
        );
        state_mut(&mut store, witch).unwrap().using_item = true;
        sync_state_metadata(store.data_mut(witch).unwrap());
        assert_eq!(
            store
                .data(witch)
                .unwrap()
                .metadata
                .get(WITCH_USING_ITEM_INDEX),
            Some(&MetaValue::Boolean(true))
        );
    }

    #[test]
    fn per_mob_flags_and_sound_variants_sync_from_state() {
        use ferrite_protocol::packets::play::MetaValue;
        let mut store = EntityStore::default();
        let mut rng = Rng::new(123);

        let goat = store.insert(new_mob(MobKind::Goat, Vec3::new(0.5, 64.0, 0.5), &mut rng));
        {
            let state = state_mut(&mut store, goat).unwrap();
            state.screaming = true;
            state.has_left_horn = false;
            state.has_right_horn = true;
        }
        sync_state_metadata(store.data_mut(goat).unwrap());
        let metadata = &store.data(goat).unwrap().metadata;
        assert_eq!(
            metadata.get(crate::metadata::goat::IS_SCREAMING),
            Some(&MetaValue::Boolean(true))
        );
        assert_eq!(
            metadata.get(crate::metadata::goat::HAS_LEFT_HORN),
            Some(&MetaValue::Boolean(false))
        );
        assert_eq!(
            metadata.get(crate::metadata::goat::HAS_RIGHT_HORN),
            Some(&MetaValue::Boolean(true))
        );

        let fox = store.insert(new_mob(MobKind::Fox, Vec3::new(1.5, 64.0, 0.5), &mut rng));
        let first = uuid::Uuid::from_u128(0x11);
        let second = uuid::Uuid::from_u128(0x22);
        {
            let state = state_mut(&mut store, fox).unwrap();
            state.sitting = true;
            state.fox_crouching = true;
            state.fox_sleeping = true;
            state.trusted = [Some(first), Some(second)];
        }
        sync_state_metadata(store.data_mut(fox).unwrap());
        let metadata = &store.data(fox).unwrap().metadata;
        assert_eq!(
            metadata.get(crate::metadata::fox::FLAGS),
            Some(&MetaValue::Byte(
                (crate::metadata::fox::FLAG_SITTING
                    | crate::metadata::fox::FLAG_CROUCHING
                    | crate::metadata::fox::FLAG_SLEEPING) as i8
            ))
        );
        assert_eq!(
            metadata.get(crate::metadata::fox::TRUSTED_0),
            Some(&MetaValue::OptionalUuid(Some(first)))
        );
        assert_eq!(
            metadata.get(crate::metadata::fox::TRUSTED_1),
            Some(&MetaValue::OptionalUuid(Some(second)))
        );

        let golem = store.insert(new_mob(
            MobKind::IronGolem,
            Vec3::new(2.5, 64.0, 0.5),
            &mut rng,
        ));
        state_mut(&mut store, golem).unwrap().player_created = true;
        sync_state_metadata(store.data_mut(golem).unwrap());
        assert_eq!(
            store
                .data(golem)
                .unwrap()
                .metadata
                .get(crate::metadata::iron_golem::FLAGS),
            Some(&MetaValue::Byte(
                crate::metadata::iron_golem::FLAG_PLAYER_CREATED
            ))
        );

        let piglin = store.insert(new_mob(
            MobKind::Piglin,
            Vec3::new(3.5, 64.0, 0.5),
            &mut rng,
        ));
        {
            let state = state_mut(&mut store, piglin).unwrap();
            state.charging_crossbow = true;
            state.dancing = true;
        }
        sync_state_metadata(store.data_mut(piglin).unwrap());
        let metadata = &store.data(piglin).unwrap().metadata;
        assert_eq!(
            metadata.get(crate::metadata::piglin::CHARGING_CROSSBOW),
            Some(&MetaValue::Boolean(true))
        );
        assert_eq!(
            metadata.get(crate::metadata::piglin::DANCING),
            Some(&MetaValue::Boolean(true))
        );

        let witch = store.insert(new_mob(MobKind::Witch, Vec3::new(3.5, 64.0, 1.5), &mut rng));
        {
            let state = state_mut(&mut store, witch).unwrap();
            state.using_item = true;
            state.celebrating = true;
        }
        sync_state_metadata(store.data_mut(witch).unwrap());
        let metadata = &store.data(witch).unwrap().metadata;
        assert_eq!(
            metadata.get(crate::metadata::witch::USING_ITEM),
            Some(&MetaValue::Boolean(true))
        );
        assert_eq!(
            metadata.get(crate::metadata::witch::CELEBRATING),
            Some(&MetaValue::Boolean(true))
        );

        let cow = store.insert(new_mob(MobKind::Cow, Vec3::new(4.5, 64.0, 0.5), &mut rng));
        state_mut(&mut store, cow).unwrap().sound_variant = Some(1);
        sync_state_metadata(store.data_mut(cow).unwrap());
        assert_eq!(
            store
                .data(cow)
                .unwrap()
                .metadata
                .get(crate::metadata::animal_variant::COW_SOUND_VARIANT),
            Some(&MetaValue::CowSoundVariant(1))
        );

        let wolf = store.insert(new_mob(MobKind::Wolf, Vec3::new(5.5, 64.0, 0.5), &mut rng));
        state_mut(&mut store, wolf).unwrap().sound_variant = Some(6);
        sync_state_metadata(store.data_mut(wolf).unwrap());
        assert_eq!(
            store
                .data(wolf)
                .unwrap()
                .metadata
                .get(crate::metadata::animal_variant::WOLF_SOUND_VARIANT),
            Some(&MetaValue::WolfSoundVariant(6))
        );
    }

    #[test]
    fn custom_name_metadata_and_piglin_zombification_and_hard_door_break() {
        use ferrite_protocol::packets::play::MetaValue;
        let mut rng = Rng::new(42);
        for _ in 0..200 {
            let z = new_mob_with_difficulty(MobKind::Zombie, Vec3::ZERO, &mut rng, false);
            assert!(!z.data.mob.unwrap().can_break_doors);
        }

        let mut store = EntityStore::new(4);
        let mut record = new_mob(MobKind::Piglin, Vec3::new(1.5, 64.0, 2.5), &mut rng);
        record.data.custom_name = Some("Barterer".into());
        sync_state_metadata(&mut record.data);
        let key = store.insert(record);

        assert_eq!(
            store
                .data(key)
                .unwrap()
                .metadata
                .get(crate::metadata::base::CUSTOM_NAME),
            Some(&MetaValue::OptionalText(Some(
                ferrite_protocol::types::NetworkText::Plain("Barterer".into())
            )))
        );

        // Immune piglin never advances conversion timer; nether resets it.
        state_mut(&mut store, key).unwrap().immune_to_zombification = true;
        sync_state_metadata(store.data_mut(key).unwrap());
        assert_eq!(
            store
                .data(key)
                .unwrap()
                .metadata
                .get(crate::metadata::piglin::IMMUNE_TO_ZOMBIFICATION),
            Some(&MetaValue::Boolean(true))
        );
        assert!(!tick_piglin_zombification(&mut store, key, false));
        assert_eq!(state(&store, key).unwrap().time_in_overworld, 0);

        state_mut(&mut store, key).unwrap().immune_to_zombification = false;
        assert!(!tick_piglin_zombification(&mut store, key, false));
        assert_eq!(state(&store, key).unwrap().time_in_overworld, 1);
        assert!(!tick_piglin_zombification(&mut store, key, true));
        assert_eq!(state(&store, key).unwrap().time_in_overworld, 0);

        for _ in 0..PIGLIN_ZOMBIFICATION_TICKS {
            assert!(!tick_piglin_zombification(&mut store, key, false));
        }
        assert!(tick_piglin_zombification(&mut store, key, false));
        assert_eq!(
            convert_zombified_piglin(&mut store, key),
            Some(MobKind::ZombifiedPiglin)
        );

        let converted = store.data(key).unwrap();
        assert_eq!(converted.mob.unwrap().kind, MobKind::ZombifiedPiglin);
        assert!(converted.piglin_inventory.is_none());
        assert_eq!(converted.custom_name.as_deref(), Some("Barterer"));
        assert!(converted.effects.has(crate::effect::Effect::Nausea));
        assert_eq!(
            converted.metadata.get(ZOMBIE_SPECIAL_TYPE_INDEX),
            Some(&MetaValue::Int(0))
        );
        assert!(converted.metadata.get(19).is_none());
    }
}
