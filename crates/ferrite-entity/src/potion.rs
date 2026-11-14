//! Potions (`net.minecraft.world.item.alchemy.Potions`) and `minecraft:potion_contents`.
//!
//! A potion item carries the `minecraft:potion_contents` component: an optional potion id from
//! the `minecraft:potion` registry, optional custom effects, a custom color and a custom name.
//! Drinking applies the potion's effects (`PotionContents.applyToLivingEntity`). Splash potions
//! are thrown projectiles (`arrow::Kind::SplashPotion`) that burst where they land; lingering
//! potions (`arrow::Kind::LingeringPotion`) leave a [`PotionCloud`], which is also spawned as a
//! `minecraft:area_effect_cloud` entity so clients see it (see [`PotionCloud::spawn_record`]).
//!
//! Durations and amplifiers mirror the vanilla `Potions` registrations; the effect ids come from
//! [`Effect`](crate::effect::Effect), and potion ids are checked against the 26.3 registry
//! report in tests.

use ferrite_data::registries;
use ferrite_nbt::{Compound, List, Tag};

use crate::effect::{Effect, MobEffectInstance};

/// One vanilla potion: the registry name and its effect list as `(effect, duration, amplifier)`.
#[derive(Debug, PartialEq)]
pub struct PotionDef {
    pub name: &'static str,
    pub effects: &'static [(Effect, i32, i32)],
}

const fn effect(effect: Effect, duration: i32, amplifier: i32) -> (Effect, i32, i32) {
    (effect, duration, amplifier)
}

/// Every potion in the 26.3 `minecraft:potion` registry, in protocol-id order.
pub const POTIONS: &[PotionDef] = &[
    PotionDef {
        name: "water",
        effects: &[],
    },
    PotionDef {
        name: "mundane",
        effects: &[],
    },
    PotionDef {
        name: "thick",
        effects: &[],
    },
    PotionDef {
        name: "awkward",
        effects: &[],
    },
    PotionDef {
        name: "night_vision",
        effects: &[effect(Effect::NightVision, 3600, 0)],
    },
    PotionDef {
        name: "long_night_vision",
        effects: &[effect(Effect::NightVision, 9600, 0)],
    },
    PotionDef {
        name: "invisibility",
        effects: &[effect(Effect::Invisibility, 3600, 0)],
    },
    PotionDef {
        name: "long_invisibility",
        effects: &[effect(Effect::Invisibility, 9600, 0)],
    },
    PotionDef {
        name: "leaping",
        effects: &[effect(Effect::JumpBoost, 3600, 0)],
    },
    PotionDef {
        name: "long_leaping",
        effects: &[effect(Effect::JumpBoost, 9600, 0)],
    },
    PotionDef {
        name: "strong_leaping",
        effects: &[effect(Effect::JumpBoost, 1800, 1)],
    },
    PotionDef {
        name: "fire_resistance",
        effects: &[effect(Effect::FireResistance, 3600, 0)],
    },
    PotionDef {
        name: "long_fire_resistance",
        effects: &[effect(Effect::FireResistance, 9600, 0)],
    },
    PotionDef {
        name: "swiftness",
        effects: &[effect(Effect::Speed, 3600, 0)],
    },
    PotionDef {
        name: "long_swiftness",
        effects: &[effect(Effect::Speed, 9600, 0)],
    },
    PotionDef {
        name: "strong_swiftness",
        effects: &[effect(Effect::Speed, 1800, 1)],
    },
    PotionDef {
        name: "slowness",
        effects: &[effect(Effect::Slowness, 1800, 0)],
    },
    PotionDef {
        name: "long_slowness",
        effects: &[effect(Effect::Slowness, 4800, 0)],
    },
    PotionDef {
        name: "strong_slowness",
        effects: &[effect(Effect::Slowness, 400, 3)],
    },
    PotionDef {
        name: "turtle_master",
        effects: &[
            effect(Effect::Slowness, 400, 3),
            effect(Effect::Resistance, 400, 2),
        ],
    },
    PotionDef {
        name: "long_turtle_master",
        effects: &[
            effect(Effect::Slowness, 800, 3),
            effect(Effect::Resistance, 800, 2),
        ],
    },
    PotionDef {
        name: "strong_turtle_master",
        effects: &[
            effect(Effect::Slowness, 400, 5),
            effect(Effect::Resistance, 400, 3),
        ],
    },
    PotionDef {
        name: "water_breathing",
        effects: &[effect(Effect::WaterBreathing, 3600, 0)],
    },
    PotionDef {
        name: "long_water_breathing",
        effects: &[effect(Effect::WaterBreathing, 9600, 0)],
    },
    PotionDef {
        name: "healing",
        effects: &[effect(Effect::InstantHealth, 1, 0)],
    },
    PotionDef {
        name: "strong_healing",
        effects: &[effect(Effect::InstantHealth, 1, 1)],
    },
    PotionDef {
        name: "harming",
        effects: &[effect(Effect::InstantDamage, 1, 0)],
    },
    PotionDef {
        name: "strong_harming",
        effects: &[effect(Effect::InstantDamage, 1, 1)],
    },
    PotionDef {
        name: "poison",
        effects: &[effect(Effect::Poison, 900, 0)],
    },
    PotionDef {
        name: "long_poison",
        effects: &[effect(Effect::Poison, 1800, 0)],
    },
    PotionDef {
        name: "strong_poison",
        effects: &[effect(Effect::Poison, 432, 1)],
    },
    PotionDef {
        name: "regeneration",
        effects: &[effect(Effect::Regeneration, 900, 0)],
    },
    PotionDef {
        name: "long_regeneration",
        effects: &[effect(Effect::Regeneration, 1800, 0)],
    },
    PotionDef {
        name: "strong_regeneration",
        effects: &[effect(Effect::Regeneration, 450, 1)],
    },
    PotionDef {
        name: "strength",
        effects: &[effect(Effect::Strength, 3600, 0)],
    },
    PotionDef {
        name: "long_strength",
        effects: &[effect(Effect::Strength, 9600, 0)],
    },
    PotionDef {
        name: "strong_strength",
        effects: &[effect(Effect::Strength, 1800, 1)],
    },
    PotionDef {
        name: "weakness",
        effects: &[effect(Effect::Weakness, 1800, 0)],
    },
    PotionDef {
        name: "long_weakness",
        effects: &[effect(Effect::Weakness, 4800, 0)],
    },
    PotionDef {
        name: "luck",
        effects: &[effect(Effect::Luck, 6000, 0)],
    },
    PotionDef {
        name: "slow_falling",
        effects: &[effect(Effect::SlowFalling, 1800, 0)],
    },
    PotionDef {
        name: "long_slow_falling",
        effects: &[effect(Effect::SlowFalling, 4800, 0)],
    },
    PotionDef {
        name: "wind_charged",
        effects: &[effect(Effect::WindCharged, 3600, 0)],
    },
    PotionDef {
        name: "weaving",
        effects: &[effect(Effect::Weaving, 3600, 0)],
    },
    PotionDef {
        name: "oozing",
        effects: &[effect(Effect::Oozing, 3600, 0)],
    },
    PotionDef {
        name: "infested",
        effects: &[effect(Effect::Infested, 3600, 0)],
    },
];

/// Looks up a potion by registry name, with or without the `minecraft:` prefix.
pub fn by_name(name: &str) -> Option<&'static PotionDef> {
    let wanted = name.strip_prefix("minecraft:").unwrap_or(name);
    POTIONS.iter().find(|potion| potion.name == wanted)
}

/// The effects of drinking a potion; `None` for an unknown potion id.
pub fn effects(name: &str) -> Option<Vec<MobEffectInstance>> {
    by_name(name).map(|potion| {
        potion
            .effects
            .iter()
            .map(|&(effect, duration, amplifier)| {
                MobEffectInstance::new(effect.id(), duration, amplifier)
            })
            .collect()
    })
}

/// Effects of a typed `minecraft:potion_contents` component, in apply order: custom effects
/// first, then the base potion's.
pub fn effects_from_component(contents: &ferrite_item::PotionContents) -> Vec<MobEffectInstance> {
    let mut applied: Vec<MobEffectInstance> = contents
        .custom_effects
        .iter()
        .filter_map(|effect| {
            registries::id("minecraft:mob_effect", &effect.effect).map(|id| MobEffectInstance {
                effect: id,
                amplifier: effect.amplifier,
                duration: effect.duration,
                ambient: effect.ambient,
                visible: effect.visible,
                show_icon: effect.show_icon,
            })
        })
        .collect();
    if let Some(potion) = &contents.potion {
        applied.extend(effects(potion).unwrap_or_default());
    }
    applied
}

/// `net.minecraft.world.item.alchemy.PotionContents`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PotionContents {
    /// A `minecraft:potion` registry name, if the component sets one.
    pub potion: Option<String>,
    pub custom_color: Option<i32>,
    pub custom_effects: Vec<MobEffectInstance>,
    pub custom_name: Option<String>,
}

impl PotionContents {
    /// Decodes the component's NBT. `{}` is valid and means "water" (no effects).
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let Tag::Compound(compound) = tag else {
            return None;
        };
        let potion = compound.get("potion").and_then(|tag| match tag {
            Tag::String(name) => Some(name.clone()),
            _ => None,
        });
        let custom_color = compound.get("custom_color").and_then(|tag| match tag {
            Tag::Int(color) => Some(*color),
            _ => None,
        });
        let custom_name = compound.get("custom_name").and_then(|tag| match tag {
            Tag::String(name) => Some(name.clone()),
            _ => None,
        });
        let custom_effects = match compound.get("custom_effects") {
            Some(Tag::List(List::Compound(effects))) => {
                effects.iter().filter_map(effect_from_nbt).collect()
            }
            _ => Vec::new(),
        };
        Some(Self {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        })
    }

    /// `PotionContents.getAllEffects`: the custom effects plus the base potion's effects.
    pub fn all_effects(&self) -> Vec<MobEffectInstance> {
        let mut effects = self.custom_effects.clone();
        if let Some(potion) = &self.potion {
            effects.extend(crate::potion::effects(potion).unwrap_or_default());
        }
        effects
    }
}

/// One `custom_effects` entry: `{id: "minecraft:speed", amplifier: 1, duration: 200, ...}`.
fn effect_from_nbt(compound: &Compound) -> Option<MobEffectInstance> {
    let id = match compound.get("id")? {
        Tag::String(id) => id.as_str(),
        _ => return None,
    };
    let effect = registries::id("minecraft:mob_effect", id)?;
    let amplifier = match compound.get("amplifier") {
        Some(tag) => int(tag)?,
        None => 0,
    };
    let duration = match compound.get("duration") {
        Some(tag) => int(tag)?,
        None => 0,
    };
    let mut instance = MobEffectInstance::new(effect, duration, amplifier);
    if let Some(Tag::Byte(value)) = compound.get("ambient") {
        instance.ambient = *value != 0;
    }
    if let Some(tag) = compound.get("show_particles") {
        instance.visible = int(tag)? != 0;
    }
    if let Some(tag) = compound.get("show_icon") {
        instance.show_icon = int(tag)? != 0;
    }
    Some(instance)
}

fn int(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(value) => Some(i32::from(*value)),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Int(value) => Some(*value),
        Tag::Long(value) => i32::try_from(*value).ok(),
        _ => None,
    }
}

/// `minecraft:entity_effect` in the 26.3 particle type registry.
const ENTITY_EFFECT_PARTICLE: i32 = 28;

/// Entity type id of `minecraft:area_effect_cloud`.
pub fn cloud_type() -> u16 {
    static KIND: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *KIND.get_or_init(|| {
        ferrite_data::entity_props::by_name("area_effect_cloud")
            .expect("vanilla has an area_effect_cloud entity type")
            .id
    })
}

/// Vanilla `AreaEffectCloud` left by a lingering potion (`ThrownPotion.makeAreaOfEffectCloud`).
///
/// It starts with a 3 block radius, waits 10 ticks, then shrinks evenly over its 600 ticks. Every
/// fifth tick it applies its effects to whoever stands inside and has not been hit during the last
/// `REAPPLICATION_DELAY` ticks; each application costs half a block of radius. It ends when it
/// has run its course or its radius falls under half a block. Effect durations are a quarter of
/// the potion's, except for instant effects. [`PotionCloud::spawn_record`] builds the entity
/// clients see and [`PotionCloud::sync`] keeps its synched radius and waiting flag current.
#[derive(Debug, Clone, PartialEq)]
pub struct PotionCloud {
    pub center: ferrite_math::Vec3,
    pub effects: Vec<MobEffectInstance>,
    /// Uuid of the client-visible entity.
    uuid: uuid::Uuid,
    /// ARGB-less RGB colour of the particle.
    color: i32,
    age: u32,
    radius: f32,
    radius_per_tick: f32,
    /// Who was hit and the tick until which they are skipped.
    victims: Vec<(uuid::Uuid, u32)>,
}

impl PotionCloud {
    pub const RADIUS: f32 = 3.0;
    pub const RADIUS_ON_USE: f32 = -0.5;
    pub const MIN_RADIUS: f32 = 0.5;
    pub const DURATION: u32 = 600;
    pub const WAIT: u32 = 10;
    pub const REAPPLICATION_DELAY: u32 = 20;
    pub const CHECK_INTERVAL: u32 = 5;
    /// Height of the cloud's own box (its dimensions are `radius * 2` by `0.5`).
    pub const BOX_HEIGHT: f64 = 0.5;
    /// Vanilla water colour, used when the bottle has no `custom_color`. Colours averaged from
    /// the effects are not modelled (no local effect colour table), so those clouds look blue.
    pub const DEFAULT_COLOR: i32 = 0x0038_5DC6;

    /// A cloud for the effects of a lingering potion bottle.
    pub fn lingering(center: ferrite_math::Vec3, potion_effects: &[MobEffectInstance]) -> Self {
        let effects = potion_effects
            .iter()
            .map(|effect| {
                let mut effect = *effect;
                if !effect.kind().is_instant() {
                    effect.duration /= 4;
                }
                effect
            })
            .collect();
        Self {
            center,
            effects,
            uuid: crate::random_uuid(),
            color: Self::DEFAULT_COLOR,
            age: 0,
            radius: Self::RADIUS,
            radius_per_tick: -Self::RADIUS / Self::DURATION as f32,
            victims: Vec::new(),
        }
    }

    /// A cloud tinted with the bottle's `custom_color`, if it has one.
    pub fn with_color(mut self, color: Option<i32>) -> Self {
        if let Some(color) = color {
            self.color = color & 0x00FF_FFFF;
        }
        self
    }

    pub fn uuid(&self) -> uuid::Uuid {
        self.uuid
    }

    pub fn color(&self) -> i32 {
        self.color
    }

    /// Whether the cloud is still in its wait phase (the synched `WAITING` flag).
    pub fn waiting(&self) -> bool {
        self.age < Self::WAIT
    }

    /// The `minecraft:area_effect_cloud` entity for this cloud. The particle is the tinted
    /// `entity_effect` particle.
    pub fn spawn_record(&self) -> crate::store::EntityRecord {
        use crate::metadata::area_effect_cloud as meta;
        use crate::store::{EntityData, EntityRecord, Flags};
        use ferrite_protocol::packets::play::MetaValue;
        let kind = cloud_type();
        let mut data = EntityData::new(
            crate::allocate_id(),
            self.uuid,
            kind,
            f64::from(Self::RADIUS) * 2.0,
            0.5,
        );
        data.metadata.define(meta::RADIUS, MetaValue::Float(0.0));
        data.metadata
            .define(meta::WAITING, MetaValue::Boolean(false));
        data.metadata.define(
            meta::PARTICLE,
            MetaValue::Particle {
                id: ENTITY_EFFECT_PARTICLE,
                argb: Some((0xFF00_0000_u32 | Self::DEFAULT_COLOR as u32) as i32),
            },
        );
        data.metadata
            .set(meta::RADIUS, MetaValue::Float(self.radius));
        data.metadata
            .set(meta::WAITING, MetaValue::Boolean(self.waiting()));
        data.metadata.set(meta::PARTICLE, self.particle());
        let mut flags = Flags::default();
        flags.set(Flags::NO_GRAVITY, true);
        flags.set(Flags::NO_PHYSICS, true);
        EntityRecord {
            pos: self.center,
            vel: ferrite_math::Vec3::ZERO,
            flags,
            data,
        }
    }

    fn particle(&self) -> ferrite_protocol::packets::play::MetaValue {
        ferrite_protocol::packets::play::MetaValue::Particle {
            id: ENTITY_EFFECT_PARTICLE,
            argb: Some((0xFF00_0000_u32 | self.color as u32) as i32),
        }
    }

    /// Pushes the current radius and waiting flag into the entity's synched data; trackers send
    /// only what changed. Does nothing when the entity is not in `store`.
    pub fn sync(&self, store: &mut crate::store::EntityStore) {
        use crate::metadata::area_effect_cloud as meta;
        use ferrite_protocol::packets::play::MetaValue;
        if let Some(key) = self.entity_key(store)
            && let Some(data) = store.data_mut(key)
        {
            data.metadata
                .set(meta::RADIUS, MetaValue::Float(self.radius));
            data.metadata
                .set(meta::WAITING, MetaValue::Boolean(self.waiting()));
        }
    }

    /// Flags the entity removed so trackers destroy it and the store sweeps it.
    pub fn despawn(&self, store: &mut crate::store::EntityStore) {
        if let Some(key) = self.entity_key(store)
            && let Some(flags) = store.flags_mut(key)
        {
            flags.set(crate::store::Flags::REMOVED, true);
        }
    }

    fn entity_key(&self, store: &crate::store::EntityStore) -> Option<crate::EntityKey> {
        store
            .keys()
            .iter()
            .copied()
            .find(|key| store.data(*key).is_some_and(|d| d.uuid == self.uuid))
    }

    pub fn age(&self) -> u32 {
        self.age
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// [`Self::tick`] for candidates of the standing human width and height.
    pub fn tick(
        &mut self,
        candidates: impl IntoIterator<Item = (uuid::Uuid, [f64; 3])>,
    ) -> Option<Vec<(uuid::Uuid, MobEffectInstance)>> {
        self.tick_boxes(candidates.into_iter().map(|(id, pos)| (id, pos, 0.6, 1.8)))
    }

    /// Advances one tick over `candidates` (uuid, feet position, width and height of everything
    /// that can be affected). Returns `None` once the cloud is gone, otherwise the effects to
    /// apply this tick as `(uuid, effect)`.
    ///
    /// Vanilla `AreaEffectCloud.serverTick`: while it waits it does nothing at all; afterwards it
    /// shrinks every tick and, on every fifth tick, hits every living entity whose bounding box
    /// intersects the cloud's radius-wide, half-block-tall box. A body can be hit while its centre
    /// is still outside the radius (its edge reaches in), which a centre-distance test misses.
    pub fn tick_boxes(
        &mut self,
        candidates: impl IntoIterator<Item = (uuid::Uuid, [f64; 3], f64, f64)>,
    ) -> Option<Vec<(uuid::Uuid, MobEffectInstance)>> {
        self.age += 1;
        if self.age >= Self::WAIT + Self::DURATION {
            return None;
        }
        let mut applied = Vec::new();
        if self.age < Self::WAIT {
            return Some(applied);
        }
        self.radius += self.radius_per_tick;
        if self.radius < Self::MIN_RADIUS {
            return None;
        }
        if !self.age.is_multiple_of(Self::CHECK_INTERVAL) {
            return Some(applied);
        }
        let now = self.age;
        self.victims.retain(|(_, until)| now < *until);
        if self.effects.is_empty() {
            return Some(applied);
        }
        for (id, pos, width, height) in candidates {
            if self.victims.iter().any(|(victim, _)| *victim == id) {
                continue;
            }
            let radius = f64::from(self.radius);
            let half = width.max(0.0) * 0.5;
            let horizontal = pos[0] + half >= self.center.x - radius
                && pos[0] - half <= self.center.x + radius
                && pos[2] + half >= self.center.z - radius
                && pos[2] - half <= self.center.z + radius;
            // The cloud's box is `Self::BOX_HEIGHT` tall from its own height up.
            let vertical = pos[1] < self.center.y + Self::BOX_HEIGHT
                && pos[1] + height.max(0.0) > self.center.y;
            if !horizontal || !vertical {
                continue;
            }
            self.victims.push((id, now + Self::REAPPLICATION_DELAY));
            applied.extend(self.effects.iter().map(|effect| (id, *effect)));
            self.radius += Self::RADIUS_ON_USE;
            if self.radius < Self::MIN_RADIUS {
                return None;
            }
        }
        Some(applied)
    }
}

/// Vanilla `AABB.distanceToSqr(AABB)`: squared gap between two boxes (0 when they touch).
pub fn box_gap_sqr(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> f64 {
    (0..3)
        .map(|axis| {
            let gap = (a.0[axis] - b.1[axis]).max(b.0[axis] - a.1[axis]).max(0.0);
            gap * gap
        })
        .sum()
}

/// Vanilla `ThrownSplashPotion.onHitAsPotion` falloff: a living entity whose box, grown by
/// `margin`, lies within four blocks of the potion's box (0.25 wide, standing on `center`) gets
/// `1 - distance / 4` of the effect; further away it gets nothing. `entity` is its box (min, max).
pub fn splash_scale(center: [f64; 3], entity: ([f64; 3], [f64; 3]), margin: f64) -> Option<f64> {
    let potion = (
        [center[0] - 0.125, center[1], center[2] - 0.125],
        [center[0] + 0.125, center[1] + 0.25, center[2] + 0.125],
    );
    let grown = (entity.0.map(|v| v - margin), entity.1.map(|v| v + margin));
    let gap = box_gap_sqr(potion, grown);
    (gap < 16.0).then(|| 1.0 - gap.sqrt() / 4.0)
}

/// What a splash of `scale` does to a lasting effect: `scale * duration + 0.5` truncated, and
/// nothing at all when that would end within a second (`MobEffectInstance.endsWithin(20)`).
pub fn splash_duration(instance: MobEffectInstance, scale: f64) -> Option<MobEffectInstance> {
    if instance.is_infinite() {
        return Some(instance);
    }
    let mut scaled = instance;
    scaled.duration = (scale * f64::from(instance.duration) + 0.5) as i32;
    (scaled.duration > 20).then_some(scaled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_nbt::{Compound, snbt};

    #[test]
    fn every_registered_potion_has_a_definition() {
        for name in [
            "water",
            "mundane",
            "thick",
            "awkward",
            "night_vision",
            "long_night_vision",
            "invisibility",
            "long_invisibility",
            "leaping",
            "long_leaping",
            "strong_leaping",
            "fire_resistance",
            "long_fire_resistance",
            "swiftness",
            "long_swiftness",
            "strong_swiftness",
            "slowness",
            "long_slowness",
            "strong_slowness",
            "turtle_master",
            "long_turtle_master",
            "strong_turtle_master",
            "water_breathing",
            "long_water_breathing",
            "healing",
            "strong_healing",
            "harming",
            "strong_harming",
            "poison",
            "long_poison",
            "strong_poison",
            "regeneration",
            "long_regeneration",
            "strong_regeneration",
            "strength",
            "long_strength",
            "strong_strength",
            "weakness",
            "long_weakness",
            "luck",
            "slow_falling",
            "long_slow_falling",
            "wind_charged",
            "weaving",
            "oozing",
            "infested",
        ] {
            assert!(
                registries::id("minecraft:potion", &format!("minecraft:{name}")).is_some(),
                "{name} is not a registered potion"
            );
            assert!(by_name(name).is_some(), "{name} has no definition");
            assert!(by_name(&format!("minecraft:{name}")).is_some());
        }
        assert_eq!(POTIONS.len(), 46);
        assert_eq!(by_name("not_a_potion"), None);
    }

    #[test]
    fn potion_effects_match_vanilla_durations() {
        assert_eq!(
            effects("swiftness").unwrap(),
            [MobEffectInstance::new(Effect::Speed.id(), 3600, 0)]
        );
        assert_eq!(
            effects("strong_swiftness").unwrap(),
            [MobEffectInstance::new(Effect::Speed.id(), 1800, 1)]
        );
        assert_eq!(
            effects("strong_slowness").unwrap(),
            [MobEffectInstance::new(Effect::Slowness.id(), 400, 3)]
        );
        assert_eq!(
            effects("turtle_master").unwrap(),
            [
                MobEffectInstance::new(Effect::Slowness.id(), 400, 3),
                MobEffectInstance::new(Effect::Resistance.id(), 400, 2),
            ]
        );
        assert!(effects("water").unwrap().is_empty());
        assert!(effects("mundane").unwrap().is_empty());
        assert!(effects("thick").unwrap().is_empty());
        assert!(effects("awkward").unwrap().is_empty());
        assert_eq!(effects("not_a_potion"), None);
    }

    #[test]
    fn potion_contents_parse_ids_custom_effects_and_colors() {
        let tag = snbt::parse(
            r#"{potion:"minecraft:swiftness",custom_color:1122867,custom_name:"Fast",custom_effects:[{id:"minecraft:jump_boost",amplifier:1,duration:200,show_particles:0b}]}"#,
        )
        .unwrap();
        let contents = PotionContents::from_tag(&tag).unwrap();
        assert_eq!(contents.potion.as_deref(), Some("minecraft:swiftness"));
        assert_eq!(contents.custom_color, Some(0x112233));
        assert_eq!(contents.custom_name.as_deref(), Some("Fast"));
        let effects = contents.all_effects();
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].effect, Effect::JumpBoost.id());
        assert_eq!(effects[0].amplifier, 1);
        assert!(!effects[0].visible);
        assert_eq!(effects[1].effect, Effect::Speed.id());
        assert_eq!(effects[1].duration, 3600);
    }

    #[test]
    fn empty_potion_contents_are_water_and_bad_shapes_are_ignored() {
        let empty = Tag::Compound(Compound::new());
        let contents = PotionContents::from_tag(&empty).unwrap();
        assert_eq!(contents, PotionContents::default());
        assert!(contents.all_effects().is_empty());
        assert_eq!(PotionContents::from_tag(&Tag::Int(1)), None);
        let unknown = snbt::parse(r#"{potion:"minecraft:not_a_potion"}"#).unwrap();
        assert!(
            PotionContents::from_tag(&unknown)
                .unwrap()
                .all_effects()
                .is_empty()
        );
    }

    fn cloud_at_origin(effects: &[MobEffectInstance]) -> PotionCloud {
        PotionCloud::lingering(ferrite_math::Vec3::new(0.0, 64.0, 0.0), effects)
    }

    fn speed(duration: i32) -> MobEffectInstance {
        MobEffectInstance::ambient(Effect::Speed.id(), duration, 0)
    }

    /// Runs the cloud until `age`, with nobody inside, and returns it.
    fn aged(mut cloud: PotionCloud, age: u32) -> PotionCloud {
        while cloud.age() < age {
            assert!(cloud.tick([]).is_some());
        }
        cloud
    }

    #[test]
    fn a_lingering_cloud_quarters_effect_durations_but_not_instant_ones() {
        let instant = MobEffectInstance::new(Effect::InstantHealth.id(), 1, 0);
        let cloud = cloud_at_origin(&[speed(3600), instant]);
        assert_eq!(cloud.effects[0].duration, 900);
        assert_eq!(cloud.effects[1].duration, 1);
    }

    #[test]
    fn a_cloud_applies_effects_every_fifth_tick_inside_its_radius() {
        let id = uuid::Uuid::from_u128(1);
        let mut cloud = cloud_at_origin(&[speed(3600)]);
        // Nothing happens while the cloud waits, not even on the fifth tick.
        for _ in 1..PotionCloud::WAIT {
            assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]), Some(Vec::new()));
        }
        let applied = cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].0, id);
        assert_eq!(applied[0].1.duration, 900);
    }

    #[test]
    fn someone_outside_the_radius_or_far_above_is_not_affected() {
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let near = uuid::Uuid::from_u128(1);
        let far = uuid::Uuid::from_u128(2);
        let high = uuid::Uuid::from_u128(3);
        let applied = cloud
            .tick([
                (near, [2.5, 64.0, 0.0]),
                (far, [3.5, 64.0, 0.0]),
                (high, [0.0, 70.0, 0.0]),
            ])
            .unwrap();
        assert_eq!(
            applied.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [near]
        );
    }

    #[test]
    fn a_hit_costs_half_a_block_and_the_victim_waits_twenty_ticks() {
        let id = uuid::Uuid::from_u128(1);
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let before = cloud.radius();
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap().len(), 1);
        let expected = before + cloud.radius_per_tick + PotionCloud::RADIUS_ON_USE;
        assert!((expected - cloud.radius()).abs() < 1e-4);
        // Ticks 15, 20 and 25 are checks too, but the victim is skipped until tick 30.
        for _ in 0..19 {
            let applied = cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap();
            assert!(applied.is_empty(), "age {}", cloud.age());
        }
        assert_eq!(cloud.age(), 29);
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap().len(), 1);
    }

    /// Regression: the cloud used to hit on tick 5 while it was still waiting; vanilla does
    /// nothing until `waitTime` (10) has passed, so the first hit is on tick 10.
    #[test]
    fn a_waiting_cloud_hits_nobody_until_tick_ten() {
        let id = uuid::Uuid::from_u128(1);
        let mut cloud = cloud_at_origin(&[speed(3600)]);
        for age in 1..PotionCloud::WAIT {
            let applied = cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap();
            assert!(applied.is_empty(), "age {age}");
        }
        assert_eq!(cloud.radius(), PotionCloud::RADIUS, "no shrinking yet");
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap().len(), 1);
    }

    /// The cloud's box is half a block tall from its own height; a body that overlaps it is hit,
    /// whatever the position of its feet.
    #[test]
    fn the_cloud_reaches_every_body_that_meets_its_half_block_box() {
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let (below, tall, above, short_below, edge) = (
            uuid::Uuid::from_u128(1),
            uuid::Uuid::from_u128(2),
            uuid::Uuid::from_u128(3),
            uuid::Uuid::from_u128(4),
            uuid::Uuid::from_u128(5),
        );
        let applied = cloud
            .tick_boxes([
                // Feet 1.5 below, head 0.3 above the cloud's floor: overlaps.
                (below, [0.0, 62.5, 0.0], 0.6, 1.8),
                // Feet 1.8 below, head just short of the floor: does not.
                (short_below, [0.0, 62.2, 0.0], 0.6, 1.8),
                // Feet 0.4 above the floor: inside the half block.
                (tall, [0.0, 64.4, 0.0], 0.6, 1.8),
                // Feet at 0.5 above is outside (the box is open at the top).
                (edge, [0.0, 64.5, 0.0], 0.6, 1.8),
                (above, [0.0, 66.0, 0.0], 0.6, 1.8),
            ])
            .unwrap();
        let hit: Vec<_> = applied.iter().map(|(id, _)| *id).collect();
        assert_eq!(hit, [below, tall]);
    }

    /// Regression: vanilla intersects the entity's whole bounding box with the cloud's box, so a
    /// 0.6-wide body whose centre is 0.2 outside the radius still overlaps and must be hit.
    #[test]
    fn a_body_edge_that_reaches_into_the_cloud_is_hit() {
        let id = uuid::Uuid::from_u128(1);
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let radius = f64::from(cloud.radius());
        let applied = cloud
            .tick_boxes([(id, [radius + 0.2, 64.0, 0.0], 0.6, 1.8)])
            .unwrap();
        assert_eq!(applied.len(), 1);

        let far = uuid::Uuid::from_u128(2);
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let applied = cloud
            .tick_boxes([(far, [radius + 0.5, 64.0, 0.0], 0.6, 1.8)])
            .unwrap();
        assert!(applied.is_empty());
    }

    #[test]
    fn a_splash_falls_off_with_the_gap_between_the_boxes_not_the_feet_distance() {
        let body = |x: f64| ([x - 0.3, 64.0, -0.3], [x + 0.3, 65.8, 0.3]);
        // Touching: the whole effect.
        assert_eq!(splash_scale([0.0, 64.0, 0.0], body(0.0), 0.3), Some(1.0));
        // A body whose box is 2 blocks away gets half.
        let gap = splash_scale([0.0, 64.0, 0.0], body(0.125 + 0.6 + 2.0), 0.3).unwrap();
        assert!((gap - 0.5).abs() < 1e-9, "{gap}");
        // Four blocks away gets nothing (and the old 0.25 floor is gone).
        assert_eq!(splash_scale([0.0, 64.0, 0.0], body(6.0), 0.3), None);
        let near_edge = splash_scale([0.0, 64.0, 0.0], body(0.125 + 0.6 + 3.9), 0.3).unwrap();
        assert!(near_edge < 0.05, "{near_edge}");
    }

    #[test]
    fn a_splash_rounds_the_duration_and_skips_what_would_end_within_a_second() {
        let speed = MobEffectInstance::new(Effect::Speed.id(), 3600, 0);
        assert_eq!(splash_duration(speed, 0.5).unwrap().duration, 1800);
        assert_eq!(splash_duration(speed, 0.333).unwrap().duration, 1199);
        assert!(
            splash_duration(speed, 0.005).is_none(),
            "18 ticks is under a second"
        );
        let long = MobEffectInstance::new(Effect::Speed.id(), 3600, 1);
        assert_eq!(splash_duration(long, 1.0).unwrap().amplifier, 1);
    }

    #[test]
    fn a_cloud_waits_ten_ticks_before_it_shrinks_and_ends_after_its_duration() {
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), PotionCloud::WAIT - 1);
        assert_eq!(cloud.radius(), PotionCloud::RADIUS);
        cloud.tick([]).unwrap();
        assert!(cloud.radius() < PotionCloud::RADIUS);
        let mut ticks = cloud.age();
        while cloud.tick([]).is_some() {
            ticks += 1;
        }
        // The radius reaches half a block before the full duration is over.
        assert!(ticks < PotionCloud::WAIT + PotionCloud::DURATION);
        assert!(ticks > PotionCloud::DURATION / 2);
    }

    #[test]
    fn enough_hits_dissolve_the_cloud() {
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 9);
        let people: Vec<_> = (0..8u128)
            .map(|n| (uuid::Uuid::from_u128(n + 1), [0.0, 64.0, 0.0]))
            .collect();
        // 3.0 - 0.5 * 6 = 0.0 < 0.5: the sixth person ends it.
        assert_eq!(cloud.tick(people), None);
    }

    #[test]
    fn a_cloud_without_effects_applies_nothing() {
        let mut cloud = aged(cloud_at_origin(&[]), 9);
        let id = uuid::Uuid::from_u128(1);
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]), Some(Vec::new()));
    }

    #[test]
    fn the_cloud_entity_is_an_area_effect_cloud_with_radius_waiting_and_a_tinted_particle() {
        use crate::metadata::area_effect_cloud as meta;
        use ferrite_protocol::packets::play::MetaValue;
        let cloud = cloud_at_origin(&[speed(3600)]).with_color(Some(0x112233));
        let record = cloud.spawn_record();
        assert_eq!(record.data.kind, cloud_type());
        assert_eq!(record.data.uuid, cloud.uuid());
        assert_eq!(record.pos, cloud.center);
        let sent = record.data.metadata.non_default();
        let get = |index| {
            sent.iter()
                .find(|e| e.index == index)
                .map(|e| e.value.clone())
        };
        assert_eq!(
            get(meta::RADIUS),
            Some(MetaValue::Float(PotionCloud::RADIUS))
        );
        assert_eq!(get(meta::WAITING), Some(MetaValue::Boolean(true)));
        assert_eq!(
            get(meta::PARTICLE),
            Some(MetaValue::Particle {
                id: ENTITY_EFFECT_PARTICLE,
                argb: Some(0xFF11_2233_u32 as i32),
            })
        );
    }

    #[test]
    fn syncing_updates_the_radius_and_ends_the_waiting_flag_and_despawn_flags_removal() {
        use crate::metadata::area_effect_cloud as meta;
        use crate::store::{EntityStore, Flags};
        use ferrite_protocol::packets::play::MetaValue;
        let mut cloud = cloud_at_origin(&[speed(3600)]);
        let mut store = EntityStore::default();
        let key = store.insert(cloud.spawn_record());
        store.data_mut(key).unwrap().metadata.pack_dirty();
        for _ in 0..PotionCloud::WAIT + 1 {
            cloud.tick([]);
        }
        cloud.sync(&mut store);
        let changed = store.data_mut(key).unwrap().metadata.pack_dirty().unwrap();
        assert!(
            changed
                .iter()
                .any(|e| e.index == meta::WAITING && e.value == MetaValue::Boolean(false))
        );
        assert!(
            changed
                .iter()
                .any(|e| e.index == meta::RADIUS && e.value == MetaValue::Float(cloud.radius()))
        );
        assert!(cloud.radius() < PotionCloud::RADIUS);
        assert!(!store.flags(key).unwrap().has(Flags::REMOVED));
        cloud.despawn(&mut store);
        assert!(store.flags(key).unwrap().has(Flags::REMOVED));
        assert_eq!(store.sweep_removed().len(), 1);
        // A cloud whose entity is gone syncs and despawns without effect.
        cloud.sync(&mut store);
        cloud.despawn(&mut store);
    }
}
