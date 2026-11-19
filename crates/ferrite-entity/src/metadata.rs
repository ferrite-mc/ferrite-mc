//! Synched entity data (vanilla `SynchedEntityData`): typed values at accessor indices, each
//! with its default and a dirty bit, so trackers send only what changed.

use ferrite_protocol::packets::play::{MetaEntry, MetaValue};

/// Accessor indices every entity defines (vanilla `Entity`).
pub mod base {
    pub const SHARED_FLAGS: u8 = 0;
    pub const AIR_SUPPLY: u8 = 1;
    pub const CUSTOM_NAME_VISIBLE: u8 = 3;
    pub const SILENT: u8 = 4;
    pub const NO_GRAVITY: u8 = 5;
    pub const POSE: u8 = 6;
    pub const TICKS_FROZEN: u8 = 7;

    /// Bit masks inside [`SHARED_FLAGS`].
    pub const FLAG_ON_FIRE: u8 = 0x01;
    pub const FLAG_CROUCHING: u8 = 0x02;
    pub const FLAG_SPRINTING: u8 = 0x08;
    pub const FLAG_SWIMMING: u8 = 0x10;
    pub const FLAG_INVISIBLE: u8 = 0x20;
    pub const FLAG_GLOWING: u8 = 0x40;
    pub const FLAG_FALL_FLYING: u8 = 0x80;
}

/// Accessor indices of living entities (vanilla `LivingEntity`).
pub mod living {
    pub const LIVING_FLAGS: u8 = 8;
    pub const HEALTH: u8 = 9;
    pub const EFFECT_AMBIENCE: u8 = 11;
    pub const ARROW_COUNT: u8 = 12;
    pub const STINGER_COUNT: u8 = 13;
    pub const SLEEPING_POS: u8 = 14;
}

/// Accessor indices of mobs (vanilla `Mob`).
pub mod mob {
    pub const MOB_FLAGS: u8 = 15;
    pub const FLAG_NO_AI: u8 = 0x01;
    pub const FLAG_LEFT_HANDED: u8 = 0x02;
    pub const FLAG_AGGRESSIVE: u8 = 0x04;
}

/// Accessor indices of players (vanilla `Avatar` + `Player` in 26.3).
pub mod player {
    pub const MAIN_HAND: u8 = 15;
    pub const SKIN_CUSTOMIZATION: u8 = 16;
    pub const ABSORPTION: u8 = 17;
    pub const SCORE: u8 = 18;
    pub const SHOULDER_PARROT_LEFT: u8 = 19;
    pub const SHOULDER_PARROT_RIGHT: u8 = 20;
}

/// Accessor indices of primed TNT (vanilla `PrimedTnt`).
pub mod tnt {
    pub const FUSE: u8 = 8;
    pub const BLOCK_STATE: u8 = 9;
}

/// Accessor indices of falling blocks (vanilla `FallingBlockEntity`).
pub mod falling_block {
    pub const START_POS: u8 = 8;
}

/// Accessor indices of firework rockets (vanilla `FireworkRocketEntity`).
pub mod firework {
    pub const ITEM: u8 = 8;
    pub const ATTACHED_TO_TARGET: u8 = 9;
    pub const SHOT_AT_ANGLE: u8 = 10;
}

/// Accessor indices of eyes of ender (vanilla `EyeOfEnder`).
pub mod eye_of_ender {
    pub const ITEM: u8 = 8;
}

/// Accessor indices of arrows and tridents (vanilla `AbstractArrow`, `Arrow`, `ThrownTrident`).
pub mod arrow {
    pub const FLAGS: u8 = 8;
    pub const PIERCE_LEVEL: u8 = 9;
    pub const IN_GROUND: u8 = 10;
    pub const EFFECT_COLOR: u8 = 11;
    pub const TRIDENT_LOYALTY: u8 = 11;
    pub const TRIDENT_FOIL: u8 = 12;

    pub const FLAG_CRITICAL: i8 = 0x01;
}

/// Accessor indices of throwable item projectiles (snowball, egg, ender pearl, potion, etc.).
pub mod thrown {
    pub const ITEM: u8 = 8;
}

/// Accessor indices of wither skulls (vanilla `WitherSkull`).
pub mod wither_skull {
    pub const DANGEROUS: u8 = 8;
}

/// Accessor indices of item entities (vanilla `ItemEntity`).
pub mod item {
    pub const ITEM: u8 = 8;
}

/// Accessor indices of experience orbs (vanilla `ExperienceOrb`).
pub mod experience {
    pub const VALUE: u8 = 8;
}

/// Accessor indices of fishing bobbers (vanilla `FishingHook`): the hooked entity's network id
/// plus one (0 for none) and whether a fish is biting.
pub mod fishing_hook {
    pub const HOOKED_ENTITY: u8 = 8;
    pub const BITING: u8 = 9;
}

/// Accessor indices of area effect clouds (vanilla `AreaEffectCloud`): radius, whether the cloud
/// is still in its wait phase, and the particle it emits. Vanilla does not synch the colour,
/// `radius_on_use`, duration or wait time separately.
pub mod area_effect_cloud {
    pub const RADIUS: u8 = 8;
    pub const WAITING: u8 = 9;
    pub const PARTICLE: u8 = 10;
}

/// Accessor indices of `Bogged` (26.3).
pub mod bogged {
    pub const SHEARED: u8 = 16;
}

/// Accessor indices of `Blaze` (26.3).
pub mod blaze {
    pub const FLAGS: u8 = 16;
    pub const FLAG_ON_FIRE: i8 = 0x01;
}

/// Accessor indices of `IronGolem` (26.3).
pub mod iron_golem {
    pub const FLAGS: u8 = 16;
    pub const FLAG_PLAYER_CREATED: i8 = 0x01;
}

/// Accessor indices of `SnowGolem` (26.3).
pub mod snow_golem {
    pub const PUMPKIN: u8 = 16;
    /// Vanilla `SnowGolem.DATA_PUMPKIN_ID` default (`0x10` = wearing pumpkin).
    pub const FLAG_HAS_PUMPKIN: i8 = 0x10;
}

/// Accessor indices of `Raider` + `Witch` (26.3).
pub mod witch {
    pub const CELEBRATING: u8 = 16;
    pub const USING_ITEM: u8 = 17;
}

/// Accessor indices of `AbstractPiglin` + `Piglin` (26.3).
pub mod piglin {
    pub const IMMUNE_TO_ZOMBIFICATION: u8 = 16;
    pub const BABY: u8 = 17;
    pub const CHARGING_CROSSBOW: u8 = 18;
    pub const DANCING: u8 = 19;
}

/// Accessor indices of `Goat` (26.3; `AgeableMob` owns 16..=17).
pub mod goat {
    pub const IS_SCREAMING: u8 = 18;
    pub const HAS_LEFT_HORN: u8 = 19;
    pub const HAS_RIGHT_HORN: u8 = 20;
}

/// Accessor indices of `Fox` (26.3; `AgeableMob` owns 16..=17).
pub mod fox {
    pub const TYPE: u8 = 18;
    pub const FLAGS: u8 = 19;
    pub const TRUSTED_0: u8 = 20;
    pub const TRUSTED_1: u8 = 21;
}

/// Accessor indices of `Rabbit` (26.3; `AgeableMob` owns 16..=17).
pub mod rabbit {
    pub const TYPE: u8 = 18;
}

/// Accessor indices of `Frog` (26.3; `AgeableMob` owns 16..=17).
pub mod frog {
    pub const VARIANT: u8 = 18;
    pub const TONGUE_TARGET: u8 = 19;
    /// `minecraft:temperate` in 26.3 `frog_variant` (`0=cold, 1=temperate, 2=warm`).
    pub const DEFAULT_VARIANT: i32 = 1;
}

/// Accessor indices of `Turtle` (26.3; `AgeableMob` owns 16..=17).
pub mod turtle {
    pub const HAS_EGG: u8 = 18;
    pub const LAYING_EGG: u8 = 19;
}

/// Accessor indices of `Bee` (26.3; `AgeableMob` owns 16..=17).
pub mod bee {
    pub const FLAGS: u8 = 18;
    pub const ANGER_END_TIME: u8 = 19;
}

/// Accessor indices of `MushroomCow` (26.3; `AgeableMob` owns 16..=17).
pub mod mooshroom {
    pub const TYPE: u8 = 18;
}

/// 26.3 data-driven variant and sound-variant accessor indices and default registry ids.
pub mod animal_variant {
    /// `minecraft:temperate` in 26.3 `chicken_variant`, `cow_variant`, and `pig_variant`
    /// (`0=cold, 1=temperate, 2=warm`).
    pub const DEFAULT_TEMPERATE: i32 = 1;
    /// `minecraft:classic` in 26.3 `chicken_sound_variant`, `cow_sound_variant`, `cat_sound_variant`.
    pub const DEFAULT_CLASSIC_ZERO: i32 = 0;
    /// `minecraft:classic` in 26.3 `pig_sound_variant` (`0=big, 1=classic, 2=mini`).
    pub const DEFAULT_PIG_SOUND_CLASSIC: i32 = 1;
    /// `minecraft:classic` in 26.3 `wolf_sound_variant`
    /// (`0=angry, 1=big, 2=classic, 3=cute, 4=grumpy, 5=puglin, 6=sad`).
    pub const DEFAULT_WOLF_SOUND_CLASSIC: i32 = 2;

    pub const CHICKEN_VARIANT: u8 = 18;
    pub const CHICKEN_SOUND_VARIANT: u8 = 19;
    pub const COW_VARIANT: u8 = 18;
    pub const COW_SOUND_VARIANT: u8 = 19;
    pub const PIG_VARIANT: u8 = 19;
    pub const PIG_SOUND_VARIANT: u8 = 20;
    pub const WOLF_SOUND_VARIANT: u8 = 24;
    pub const CAT_SOUND_VARIANT: u8 = 24;
}

#[derive(Debug, Clone, PartialEq)]
struct Item {
    index: u8,
    value: MetaValue,
    default: MetaValue,
    dirty: bool,
}

/// The data values of one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    items: Vec<Item>,
    dirty: bool,
}

impl Default for Metadata {
    /// The values every entity defines. Vanilla's index 2 (optional custom name) is omitted
    /// until components are supported; it is never non-default, so nothing is lost on the wire.
    fn default() -> Self {
        let mut data = Self {
            items: Vec::new(),
            dirty: false,
        };
        data.define(base::SHARED_FLAGS, MetaValue::Byte(0));
        data.define(base::AIR_SUPPLY, MetaValue::Int(300));
        data.define(base::CUSTOM_NAME_VISIBLE, MetaValue::Boolean(false));
        data.define(base::SILENT, MetaValue::Boolean(false));
        data.define(base::NO_GRAVITY, MetaValue::Boolean(false));
        data.define(base::POSE, MetaValue::Pose(0));
        data.define(base::TICKS_FROZEN, MetaValue::Int(0));
        data
    }
}

impl Metadata {
    /// Defines an accessor with its default value; redefining an index replaces it.
    pub fn define(&mut self, index: u8, default: MetaValue) {
        let item = Item {
            index,
            value: default.clone(),
            default,
            dirty: false,
        };
        match self.items.binary_search_by_key(&index, |i| i.index) {
            Ok(at) => self.items[at] = item,
            Err(at) => self.items.insert(at, item),
        }
    }

    pub fn get(&self, index: u8) -> Option<&MetaValue> {
        self.items
            .binary_search_by_key(&index, |i| i.index)
            .ok()
            .map(|at| &self.items[at].value)
    }

    /// Sets a defined value; a changed value becomes dirty. Undefined indices are ignored.
    pub fn set(&mut self, index: u8, value: MetaValue) {
        if let Ok(at) = self.items.binary_search_by_key(&index, |i| i.index) {
            let item = &mut self.items[at];
            if item.value != value {
                item.value = value;
                item.dirty = true;
                self.dirty = true;
            }
        }
    }

    /// Defines the standard `LivingEntity` metadata indices with `max_health`.
    pub fn define_living(&mut self, max_health: f32) {
        self.define(living::LIVING_FLAGS, MetaValue::Byte(0));
        self.define(living::HEALTH, MetaValue::Float(max_health));
        self.define(living::EFFECT_AMBIENCE, MetaValue::Boolean(false));
        self.define(living::ARROW_COUNT, MetaValue::Int(0));
        self.define(living::STINGER_COUNT, MetaValue::Int(0));
        self.define(living::SLEEPING_POS, MetaValue::OptionalBlockPos(None));
    }

    /// Defines `LivingEntity`, `Mob`, and 26.3 per-type defaults for `kind`.
    pub fn define_mob(&mut self, kind: crate::mob::MobKind) {
        use crate::mob::MobKind;
        self.define_living(kind.max_health());
        self.define(mob::MOB_FLAGS, MetaValue::Byte(0));
        match kind {
            MobKind::Creeper => {
                self.define(crate::creeper::SWELL_DIR_INDEX, MetaValue::Int(-1));
                self.define(crate::creeper::POWERED_INDEX, MetaValue::Boolean(false));
                self.define(crate::creeper::IGNITED_INDEX, MetaValue::Boolean(false));
            }
            MobKind::Enderman => {
                self.define(
                    crate::enderman::CARRIED_BLOCK_INDEX,
                    MetaValue::OptionalBlockState(None),
                );
                self.define(crate::enderman::CREEPY_INDEX, MetaValue::Boolean(false));
                self.define(crate::enderman::STARED_AT_INDEX, MetaValue::Boolean(false));
            }
            MobKind::Zombie | MobKind::Drowned | MobKind::Husk | MobKind::ZombifiedPiglin => {
                self.define(crate::mob::ZOMBIE_BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::mob::ZOMBIE_SPECIAL_TYPE_INDEX, MetaValue::Int(0));
                self.define(
                    crate::mob::ZOMBIE_DROWNED_CONVERSION_INDEX,
                    MetaValue::Boolean(false),
                );
            }
            MobKind::Skeleton => {
                self.define(
                    crate::mob::SKELETON_STRAY_CONVERSION_INDEX,
                    MetaValue::Boolean(false),
                );
            }
            MobKind::Stray | MobKind::WitherSkeleton => {}
            MobKind::Bogged => {
                self.define(bogged::SHEARED, MetaValue::Boolean(false));
            }
            MobKind::Spider | MobKind::CaveSpider => {
                self.define(crate::spider::FLAGS_INDEX, MetaValue::Byte(0));
            }
            MobKind::Blaze => {
                self.define(blaze::FLAGS, MetaValue::Byte(0));
            }
            MobKind::IronGolem => {
                self.define(iron_golem::FLAGS, MetaValue::Byte(0));
            }
            MobKind::SnowGolem => {
                self.define(
                    snow_golem::PUMPKIN,
                    MetaValue::Byte(snow_golem::FLAG_HAS_PUMPKIN),
                );
            }
            MobKind::Witch => {
                self.define(witch::CELEBRATING, MetaValue::Boolean(false));
                self.define(witch::USING_ITEM, MetaValue::Boolean(false));
            }
            MobKind::Piglin => {
                self.define(piglin::IMMUNE_TO_ZOMBIFICATION, MetaValue::Boolean(false));
                self.define(piglin::BABY, MetaValue::Boolean(false));
                self.define(piglin::CHARGING_CROSSBOW, MetaValue::Boolean(false));
                self.define(piglin::DANCING, MetaValue::Boolean(false));
            }
            MobKind::EnderDragon => {
                self.define(crate::dragon::PHASE_INDEX, MetaValue::Int(0));
            }
            MobKind::Sheep => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::sheep::WOOL_INDEX, MetaValue::Byte(0));
            }
            MobKind::Pig => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::pig::BOOST_TIME_INDEX, MetaValue::Int(0));
                self.define(
                    animal_variant::PIG_VARIANT,
                    MetaValue::PigVariant(animal_variant::DEFAULT_TEMPERATE),
                );
                self.define(
                    animal_variant::PIG_SOUND_VARIANT,
                    MetaValue::PigSoundVariant(animal_variant::DEFAULT_PIG_SOUND_CLASSIC),
                );
            }
            MobKind::Cow => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(
                    animal_variant::COW_VARIANT,
                    MetaValue::CowVariant(animal_variant::DEFAULT_TEMPERATE),
                );
                self.define(
                    animal_variant::COW_SOUND_VARIANT,
                    MetaValue::CowSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
                );
            }
            MobKind::Chicken => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(
                    animal_variant::CHICKEN_VARIANT,
                    MetaValue::ChickenVariant(animal_variant::DEFAULT_TEMPERATE),
                );
                self.define(
                    animal_variant::CHICKEN_SOUND_VARIANT,
                    MetaValue::ChickenSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
                );
            }
            MobKind::Goat => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(goat::IS_SCREAMING, MetaValue::Boolean(false));
                self.define(goat::HAS_LEFT_HORN, MetaValue::Boolean(true));
                self.define(goat::HAS_RIGHT_HORN, MetaValue::Boolean(true));
            }
            MobKind::Fox => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(fox::TYPE, MetaValue::Int(0));
                self.define(fox::FLAGS, MetaValue::Byte(0));
                self.define(fox::TRUSTED_0, MetaValue::OptionalUuid(None));
                self.define(fox::TRUSTED_1, MetaValue::OptionalUuid(None));
            }
            MobKind::Rabbit => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(rabbit::TYPE, MetaValue::Int(0));
            }
            MobKind::Frog => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(frog::VARIANT, MetaValue::FrogVariant(frog::DEFAULT_VARIANT));
                self.define(frog::TONGUE_TARGET, MetaValue::OptionalUnsignedInt(None));
            }
            MobKind::Turtle => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(turtle::HAS_EGG, MetaValue::Boolean(false));
                self.define(turtle::LAYING_EGG, MetaValue::Boolean(false));
            }
            MobKind::Bee => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(bee::FLAGS, MetaValue::Byte(0));
                self.define(bee::ANGER_END_TIME, MetaValue::Long(-1));
            }
            MobKind::Mooshroom => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(mooshroom::TYPE, MetaValue::Int(0));
            }
            MobKind::Wolf => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::tame::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::tame::OWNER_INDEX, MetaValue::OptionalUuid(None));
                self.define(
                    crate::tame::WOLF_INTERESTED_INDEX,
                    MetaValue::Boolean(false),
                );
                self.define(
                    crate::tame::WOLF_COLLAR_COLOR_INDEX,
                    MetaValue::Int(crate::tame::DEFAULT_COLLAR_COLOR),
                );
                self.define(crate::tame::WOLF_ANGER_END_TIME_INDEX, MetaValue::Long(-1));
                self.define(
                    crate::tame::WOLF_VARIANT_INDEX,
                    MetaValue::WolfVariant(crate::tame::DEFAULT_WOLF_VARIANT),
                );
                self.define(
                    animal_variant::WOLF_SOUND_VARIANT,
                    MetaValue::WolfSoundVariant(animal_variant::DEFAULT_WOLF_SOUND_CLASSIC),
                );
            }
            MobKind::Cat => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::tame::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::tame::OWNER_INDEX, MetaValue::OptionalUuid(None));
                self.define(
                    crate::tame::CAT_VARIANT_INDEX,
                    MetaValue::CatVariant(crate::tame::DEFAULT_CAT_VARIANT),
                );
                self.define(crate::tame::CAT_IS_LYING_INDEX, MetaValue::Boolean(false));
                self.define(
                    crate::tame::CAT_RELAX_STATE_ONE_INDEX,
                    MetaValue::Boolean(false),
                );
                self.define(
                    crate::tame::CAT_COLLAR_COLOR_INDEX,
                    MetaValue::Int(crate::tame::DEFAULT_COLLAR_COLOR),
                );
                self.define(
                    animal_variant::CAT_SOUND_VARIANT,
                    MetaValue::CatSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
                );
            }
            MobKind::Parrot => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::tame::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::tame::OWNER_INDEX, MetaValue::OptionalUuid(None));
                self.define(crate::tame::PARROT_VARIANT_INDEX, MetaValue::Int(0));
            }
            MobKind::Horse => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::horse::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::horse::VARIANT_INDEX, MetaValue::Int(0));
            }
            MobKind::Donkey | MobKind::Mule => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::horse::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::horse::VARIANT_INDEX, MetaValue::Boolean(false));
            }
            MobKind::Llama | MobKind::TraderLlama => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::horse::FLAGS_INDEX, MetaValue::Byte(0));
                self.define(crate::horse::VARIANT_INDEX, MetaValue::Boolean(false));
                self.define(crate::horse::STRENGTH_INDEX, MetaValue::Int(0));
                self.define(crate::horse::LLAMA_VARIANT_INDEX, MetaValue::Int(0));
            }
            MobKind::Villager => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::villager::UNHAPPY_COUNTER_INDEX, MetaValue::Int(0));
                self.define(
                    crate::villager::VILLAGER_DATA_INDEX,
                    MetaValue::VillagerData {
                        kind: crate::villager::VillagerType::Plains.id(),
                        profession: crate::villager::Profession::None.id(),
                        level: 1,
                    },
                );
                self.define(
                    crate::villager::VILLAGER_DATA_FINALIZED_INDEX,
                    MetaValue::Boolean(false),
                );
            }
            MobKind::WanderingTrader => {
                self.define(crate::breed::BABY_INDEX, MetaValue::Boolean(false));
                self.define(crate::breed::AGE_LOCKED_INDEX, MetaValue::Boolean(false));
                self.define(crate::villager::UNHAPPY_COUNTER_INDEX, MetaValue::Int(0));
            }
        }
    }

    /// Builds default 26.3 metadata for an online player.
    pub fn for_player() -> Self {
        let mut meta = Self::default();
        meta.define_living(20.0);
        meta.define(player::MAIN_HAND, MetaValue::HumanoidArm(1));
        meta.define(player::SKIN_CUSTOMIZATION, MetaValue::Byte(0));
        meta.define(player::ABSORPTION, MetaValue::Float(0.0));
        meta.define(player::SCORE, MetaValue::Int(0));
        meta.define(
            player::SHOULDER_PARROT_LEFT,
            MetaValue::OptionalUnsignedInt(None),
        );
        meta.define(
            player::SHOULDER_PARROT_RIGHT,
            MetaValue::OptionalUnsignedInt(None),
        );
        meta
    }

    /// Updates one bit in [`base::SHARED_FLAGS`].
    pub fn set_shared_flag(&mut self, bit: u8, enabled: bool) {
        let current = match self.get(base::SHARED_FLAGS) {
            Some(MetaValue::Byte(flags)) => *flags as u8,
            _ => 0,
        };
        let next = if enabled {
            current | bit
        } else {
            current & !bit
        };
        self.set(base::SHARED_FLAGS, MetaValue::Byte(next as i8));
    }

    /// Reads one bit in [`base::SHARED_FLAGS`].
    pub fn shared_flag(&self, bit: u8) -> bool {
        match self.get(base::SHARED_FLAGS) {
            Some(MetaValue::Byte(flags)) => ((*flags as u8) & bit) != 0,
            _ => false,
        }
    }

    /// Updates [`base::POSE`].
    pub fn set_pose(&mut self, pose_id: i32) {
        self.set(base::POSE, MetaValue::Pose(pose_id));
    }

    /// Reads [`base::POSE`], defaulting to `0` (STANDING).
    pub fn pose(&self) -> i32 {
        match self.get(base::POSE) {
            Some(MetaValue::Pose(pose_id)) => *pose_id,
            _ => 0,
        }
    }

    /// Updates [`base::CUSTOM_NAME_VISIBLE`].
    pub fn set_custom_name_visible(&mut self, visible: bool) {
        self.set(base::CUSTOM_NAME_VISIBLE, MetaValue::Boolean(visible));
    }

    /// Updates [`living::HEALTH`] when defined.
    pub fn set_health(&mut self, health: f32) {
        self.set(living::HEALTH, MetaValue::Float(health));
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clears all dirty bits without emitting packets (used after restoring from save NBT).
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        for item in &mut self.items {
            item.dirty = false;
        }
    }

    /// Values changed since the last call, clearing their dirty bits (vanilla `packDirty`).
    pub fn pack_dirty(&mut self) -> Option<Vec<MetaEntry>> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let out = self
            .items
            .iter_mut()
            .filter_map(|i| std::mem::take(&mut i.dirty).then_some(&*i))
            .map(|i| MetaEntry {
                index: i.index,
                value: i.value.clone(),
            })
            .collect();
        Some(out)
    }

    /// Values that differ from their defaults, sent when a player starts tracking the entity.
    pub fn non_default(&self) -> Vec<MetaEntry> {
        self.items
            .iter()
            .filter(|i| i.value != i.default)
            .map(|i| MetaEntry {
                index: i.index,
                value: i.value.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::packets::play::ItemStack;

    #[test]
    fn only_changed_values_are_packed() {
        let mut data = Metadata::default();
        data.define(item::ITEM, MetaValue::ItemStack(ItemStack::default()));
        assert!(data.non_default().is_empty());
        assert_eq!(data.pack_dirty(), None);
        let stone = MetaValue::ItemStack(ItemStack {
            count: 1,
            item_id: 1,
        });
        data.set(item::ITEM, stone.clone());
        data.set(base::NO_GRAVITY, MetaValue::Boolean(false));
        let packed = data.pack_dirty().unwrap();
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].index, item::ITEM);
        assert_eq!(data.pack_dirty(), None);
        // Setting the same value again does not dirty it.
        data.set(item::ITEM, stone);
        assert!(!data.is_dirty());
        assert_eq!(data.non_default().len(), 1);
    }

    #[test]
    fn living_mob_and_player_defaults_define_expected_indices() {
        let mut creeper = Metadata::default();
        creeper.define_mob(crate::mob::MobKind::Creeper);
        assert_eq!(creeper.get(living::HEALTH), Some(&MetaValue::Float(20.0)));
        assert_eq!(creeper.get(mob::MOB_FLAGS), Some(&MetaValue::Byte(0)));
        assert_eq!(creeper.get(16), Some(&MetaValue::Int(-1)));

        let mut sheep = Metadata::default();
        sheep.define_mob(crate::mob::MobKind::Sheep);
        assert_eq!(sheep.get(living::HEALTH), Some(&MetaValue::Float(8.0)));
        assert_eq!(
            sheep.get(crate::breed::AGE_LOCKED_INDEX),
            Some(&MetaValue::Boolean(false))
        );
        assert_eq!(
            sheep.get(crate::sheep::WOOL_INDEX),
            Some(&MetaValue::Byte(0))
        );

        let mut cat = Metadata::default();
        cat.define_mob(crate::mob::MobKind::Cat);
        assert_eq!(
            cat.get(crate::tame::CAT_VARIANT_INDEX),
            Some(&MetaValue::CatVariant(crate::tame::DEFAULT_CAT_VARIANT))
        );
        assert_eq!(
            cat.get(crate::tame::CAT_COLLAR_COLOR_INDEX),
            Some(&MetaValue::Int(crate::tame::DEFAULT_COLLAR_COLOR))
        );

        let mut horse = Metadata::default();
        horse.define_mob(crate::mob::MobKind::Horse);
        assert_eq!(
            horse.get(crate::horse::FLAGS_INDEX),
            Some(&MetaValue::Byte(0))
        );
        assert_eq!(
            horse.get(crate::horse::VARIANT_INDEX),
            Some(&MetaValue::Int(0))
        );

        let mut trader = Metadata::default();
        trader.define_mob(crate::mob::MobKind::WanderingTrader);
        assert_eq!(
            trader.get(crate::villager::UNHAPPY_COUNTER_INDEX),
            Some(&MetaValue::Int(0))
        );
        assert_eq!(trader.get(crate::villager::VILLAGER_DATA_INDEX), None);

        let mut player = Metadata::for_player();
        assert_eq!(
            player.get(player::MAIN_HAND),
            Some(&MetaValue::HumanoidArm(1))
        );
        assert_eq!(
            player.get(player::SHOULDER_PARROT_LEFT),
            Some(&MetaValue::OptionalUnsignedInt(None))
        );
        player.set_shared_flag(base::FLAG_GLOWING, true);
        assert!(player.shared_flag(base::FLAG_GLOWING));
        player.set_pose(5);
        assert_eq!(player.pose(), 5);
        player.set_custom_name_visible(true);
        player.set_health(15.5);
        let dirty = player.pack_dirty().unwrap();
        assert_eq!(dirty.len(), 4);
    }

    #[test]
    fn all_39_mob_kinds_define_verified_26_3_indices_and_serializers() {
        use crate::mob::MobKind;
        assert_eq!(MobKind::ALL_MOBS.len(), 39);
        for kind in MobKind::ALL_MOBS {
            let mut m = Metadata::default();
            m.define_mob(kind);
            assert!(
                m.non_default().is_empty(),
                "{kind:?} should have no non-default entries on define_mob"
            );
            assert!(m.get(living::HEALTH).is_some(), "{kind:?} missing HEALTH");
            assert!(
                m.get(mob::MOB_FLAGS).is_some(),
                "{kind:?} missing MOB_FLAGS"
            );
        }

        let check = |kind: MobKind, idx: u8, expected: MetaValue, expected_ser: i32| {
            let mut m = Metadata::default();
            m.define_mob(kind);
            let actual = m
                .get(idx)
                .unwrap_or_else(|| panic!("{kind:?} missing index {idx}"));
            assert_eq!(actual, &expected, "{kind:?} index {idx}");
            assert_eq!(
                actual.serializer(),
                expected_ser,
                "{kind:?} index {idx} serializer"
            );
        };

        // Newly audited per-type indices, defaults, and serializers (26.3 jar).
        check(
            MobKind::Bogged,
            bogged::SHEARED,
            MetaValue::Boolean(false),
            8,
        );
        check(MobKind::Blaze, blaze::FLAGS, MetaValue::Byte(0), 0);
        check(MobKind::IronGolem, iron_golem::FLAGS, MetaValue::Byte(0), 0);
        check(
            MobKind::SnowGolem,
            snow_golem::PUMPKIN,
            MetaValue::Byte(snow_golem::FLAG_HAS_PUMPKIN),
            0,
        );
        check(
            MobKind::Witch,
            witch::CELEBRATING,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Witch,
            witch::USING_ITEM,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Piglin,
            piglin::IMMUNE_TO_ZOMBIFICATION,
            MetaValue::Boolean(false),
            8,
        );
        check(MobKind::Piglin, piglin::BABY, MetaValue::Boolean(false), 8);
        check(
            MobKind::Piglin,
            piglin::CHARGING_CROSSBOW,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Piglin,
            piglin::DANCING,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Goat,
            goat::IS_SCREAMING,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Goat,
            goat::HAS_LEFT_HORN,
            MetaValue::Boolean(true),
            8,
        );
        check(
            MobKind::Goat,
            goat::HAS_RIGHT_HORN,
            MetaValue::Boolean(true),
            8,
        );
        check(MobKind::Fox, fox::TYPE, MetaValue::Int(0), 1);
        check(MobKind::Fox, fox::FLAGS, MetaValue::Byte(0), 0);
        check(
            MobKind::Fox,
            fox::TRUSTED_0,
            MetaValue::OptionalUuid(None),
            13,
        );
        check(
            MobKind::Fox,
            fox::TRUSTED_1,
            MetaValue::OptionalUuid(None),
            13,
        );
        check(MobKind::Rabbit, rabbit::TYPE, MetaValue::Int(0), 1);
        check(
            MobKind::Frog,
            frog::VARIANT,
            MetaValue::FrogVariant(frog::DEFAULT_VARIANT),
            27,
        );
        check(
            MobKind::Frog,
            frog::TONGUE_TARGET,
            MetaValue::OptionalUnsignedInt(None),
            19,
        );
        check(
            MobKind::Turtle,
            turtle::HAS_EGG,
            MetaValue::Boolean(false),
            8,
        );
        check(
            MobKind::Turtle,
            turtle::LAYING_EGG,
            MetaValue::Boolean(false),
            8,
        );
        check(MobKind::Bee, bee::FLAGS, MetaValue::Byte(0), 0);
        check(MobKind::Bee, bee::ANGER_END_TIME, MetaValue::Long(-1), 2);
        check(MobKind::Mooshroom, mooshroom::TYPE, MetaValue::Int(0), 1);
        check(
            MobKind::Chicken,
            animal_variant::CHICKEN_VARIANT,
            MetaValue::ChickenVariant(animal_variant::DEFAULT_TEMPERATE),
            30,
        );
        check(
            MobKind::Chicken,
            animal_variant::CHICKEN_SOUND_VARIANT,
            MetaValue::ChickenSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
            31,
        );
        check(
            MobKind::Cow,
            animal_variant::COW_VARIANT,
            MetaValue::CowVariant(animal_variant::DEFAULT_TEMPERATE),
            23,
        );
        check(
            MobKind::Cow,
            animal_variant::COW_SOUND_VARIANT,
            MetaValue::CowSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
            24,
        );
        check(
            MobKind::Pig,
            animal_variant::PIG_VARIANT,
            MetaValue::PigVariant(animal_variant::DEFAULT_TEMPERATE),
            28,
        );
        check(
            MobKind::Pig,
            animal_variant::PIG_SOUND_VARIANT,
            MetaValue::PigSoundVariant(animal_variant::DEFAULT_PIG_SOUND_CLASSIC),
            29,
        );
        check(
            MobKind::Wolf,
            animal_variant::WOLF_SOUND_VARIANT,
            MetaValue::WolfSoundVariant(animal_variant::DEFAULT_WOLF_SOUND_CLASSIC),
            26,
        );
        check(
            MobKind::Cat,
            animal_variant::CAT_SOUND_VARIANT,
            MetaValue::CatSoundVariant(animal_variant::DEFAULT_CLASSIC_ZERO),
            22,
        );
    }
}
