//! Potions (`net.minecraft.world.item.alchemy.Potions`) and `minecraft:potion_contents`.
//!
//! A potion item carries the `minecraft:potion_contents` component: an optional potion id from
//! the `minecraft:potion` registry, optional custom effects, a custom color and a custom name.
//! Drinking applies the potion's effects (`PotionContents.applyToLivingEntity`). Splash potions
//! are thrown projectiles (`arrow::Kind::SplashPotion`) that burst where they land; lingering
//! potions (`arrow::Kind::LingeringPotion`) leave a [`PotionCloud`], which is simulated on the
//! server only: no area-effect-cloud entity is sent to clients yet.
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

/// Vanilla `AreaEffectCloud` left by a lingering potion (`ThrownPotion.makeAreaOfEffectCloud`).
///
/// It starts with a 3 block radius, waits 10 ticks, then shrinks evenly over its 600 ticks. Every
/// fifth tick it applies its effects to whoever stands inside and has not been hit during the last
/// `REAPPLICATION_DELAY` ticks; each application costs half a block of radius. It ends when it
/// has run its course or its radius falls under half a block. Effect durations are a quarter of
/// the potion's, except for instant effects. Only the server side is modelled: no entity is sent
/// to clients.
#[derive(Debug, Clone, PartialEq)]
pub struct PotionCloud {
    pub center: ferrite_math::Vec3,
    pub effects: Vec<MobEffectInstance>,
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
            age: 0,
            radius: Self::RADIUS,
            radius_per_tick: -Self::RADIUS / Self::DURATION as f32,
            victims: Vec::new(),
        }
    }

    pub fn age(&self) -> u32 {
        self.age
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// Advances one tick over `candidates` (uuid and feet position of everything that can be
    /// affected). Returns `None` once the cloud is gone, otherwise the effects to apply this
    /// tick as `(uuid, effect)`.
    pub fn tick(
        &mut self,
        candidates: impl IntoIterator<Item = (uuid::Uuid, [f64; 3])>,
    ) -> Option<Vec<(uuid::Uuid, MobEffectInstance)>> {
        self.age += 1;
        if self.age >= Self::WAIT + Self::DURATION {
            return None;
        }
        if self.age >= Self::WAIT {
            self.radius += self.radius_per_tick;
            if self.radius < Self::MIN_RADIUS {
                return None;
            }
        }
        let mut applied = Vec::new();
        if !self.age.is_multiple_of(Self::CHECK_INTERVAL) {
            return Some(applied);
        }
        let now = self.age;
        self.victims.retain(|(_, until)| now < *until);
        if self.effects.is_empty() {
            return Some(applied);
        }
        for (id, pos) in candidates {
            if self.victims.iter().any(|(victim, _)| *victim == id) {
                continue;
            }
            let dx = pos[0] - self.center.x;
            let dz = pos[2] - self.center.z;
            let radius = f64::from(self.radius);
            // Vanilla looks inside a box one block tall around the cloud and tests the
            // horizontal distance against the radius.
            if dx * dx + dz * dz > radius * radius
                || !(-1.0..=2.0).contains(&(pos[1] - self.center.y))
            {
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
        for _ in 1..PotionCloud::CHECK_INTERVAL {
            assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]), Some(Vec::new()));
        }
        let applied = cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].0, id);
        assert_eq!(applied[0].1.duration, 900);
    }

    #[test]
    fn someone_outside_the_radius_or_far_above_is_not_affected() {
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 4);
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
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 4);
        let before = cloud.radius();
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap().len(), 1);
        assert!((before + PotionCloud::RADIUS_ON_USE - cloud.radius()).abs() < 1e-4);
        // Ticks 10, 15 and 20 are checks too, but the victim is skipped until tick 25.
        for _ in 0..19 {
            let applied = cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap();
            assert!(applied.is_empty(), "age {}", cloud.age());
        }
        assert_eq!(cloud.age(), 24);
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]).unwrap().len(), 1);
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
        let mut cloud = aged(cloud_at_origin(&[speed(3600)]), 4);
        let people: Vec<_> = (0..8u128)
            .map(|n| (uuid::Uuid::from_u128(n + 1), [0.0, 64.0, 0.0]))
            .collect();
        // 3.0 - 0.5 * 6 = 0.0 < 0.5: the sixth person ends it.
        assert_eq!(cloud.tick(people), None);
    }

    #[test]
    fn a_cloud_without_effects_applies_nothing() {
        let mut cloud = aged(cloud_at_origin(&[]), 4);
        let id = uuid::Uuid::from_u128(1);
        assert_eq!(cloud.tick([(id, [0.0, 64.0, 0.0])]), Some(Vec::new()));
    }
}
