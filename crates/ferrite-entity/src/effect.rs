//! Mob effects (vanilla `MobEffectInstance` + `LivingEntity.activeEffects`).
//!
//! The protocol side is `update_mob_effect`/`remove_mob_effect`; this module owns the instance
//! model, the vanilla timing of the ticked effects, and the attribute modifiers that feed
//! movement speed, attack damage, maximum health and mining speed.
//!
//! Effect ids are the `minecraft:mob_effect` protocol ids from the 26.3 registry report.

use ferrite_data::registries;

/// `MobEffectInstance.INFINITE_DURATION`.
pub const INFINITE: i32 = -1;

/// A vanilla mob effect, with the protocol ids of the 26.3 `minecraft:mob_effect` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    Speed,
    Slowness,
    Haste,
    MiningFatigue,
    Strength,
    InstantHealth,
    InstantDamage,
    JumpBoost,
    Nausea,
    Regeneration,
    Resistance,
    FireResistance,
    WaterBreathing,
    Invisibility,
    Blindness,
    NightVision,
    Hunger,
    Weakness,
    Poison,
    Wither,
    HealthBoost,
    Absorption,
    Saturation,
    Glowing,
    Levitation,
    Luck,
    SlowFalling,
    Darkness,
    WindCharged,
    Weaving,
    Oozing,
    Infested,
    /// An effect added by a newer data pack; its id is preserved.
    Unknown(i32),
}

impl Effect {
    pub const ALL: [Self; 32] = [
        Self::Speed,
        Self::Slowness,
        Self::Haste,
        Self::MiningFatigue,
        Self::Strength,
        Self::InstantHealth,
        Self::InstantDamage,
        Self::JumpBoost,
        Self::Nausea,
        Self::Regeneration,
        Self::Resistance,
        Self::FireResistance,
        Self::WaterBreathing,
        Self::Invisibility,
        Self::Blindness,
        Self::NightVision,
        Self::Hunger,
        Self::Weakness,
        Self::Poison,
        Self::Wither,
        Self::HealthBoost,
        Self::Absorption,
        Self::Saturation,
        Self::Glowing,
        Self::Levitation,
        Self::Luck,
        Self::SlowFalling,
        Self::Darkness,
        Self::WindCharged,
        Self::Weaving,
        Self::Oozing,
        Self::Infested,
    ];

    pub fn from_id(id: i32) -> Self {
        match id {
            0 => Self::Speed,
            1 => Self::Slowness,
            2 => Self::Haste,
            3 => Self::MiningFatigue,
            4 => Self::Strength,
            5 => Self::InstantHealth,
            6 => Self::InstantDamage,
            7 => Self::JumpBoost,
            8 => Self::Nausea,
            9 => Self::Regeneration,
            10 => Self::Resistance,
            11 => Self::FireResistance,
            12 => Self::WaterBreathing,
            13 => Self::Invisibility,
            14 => Self::Blindness,
            15 => Self::NightVision,
            16 => Self::Hunger,
            17 => Self::Weakness,
            18 => Self::Poison,
            19 => Self::Wither,
            20 => Self::HealthBoost,
            21 => Self::Absorption,
            22 => Self::Saturation,
            23 => Self::Glowing,
            24 => Self::Levitation,
            25 => Self::Luck,
            27 => Self::SlowFalling,
            32 => Self::Darkness,
            35 => Self::WindCharged,
            36 => Self::Weaving,
            37 => Self::Oozing,
            38 => Self::Infested,
            other => Self::Unknown(other),
        }
    }

    pub fn id(self) -> i32 {
        match self {
            Self::Speed => 0,
            Self::Slowness => 1,
            Self::Haste => 2,
            Self::MiningFatigue => 3,
            Self::Strength => 4,
            Self::InstantHealth => 5,
            Self::InstantDamage => 6,
            Self::JumpBoost => 7,
            Self::Nausea => 8,
            Self::Regeneration => 9,
            Self::Resistance => 10,
            Self::FireResistance => 11,
            Self::WaterBreathing => 12,
            Self::Invisibility => 13,
            Self::Blindness => 14,
            Self::NightVision => 15,
            Self::Hunger => 16,
            Self::Weakness => 17,
            Self::Poison => 18,
            Self::Wither => 19,
            Self::HealthBoost => 20,
            Self::Absorption => 21,
            Self::Saturation => 22,
            Self::Glowing => 23,
            Self::Levitation => 24,
            Self::Luck => 25,
            Self::SlowFalling => 27,
            Self::Darkness => 32,
            Self::WindCharged => 35,
            Self::Weaving => 36,
            Self::Oozing => 37,
            Self::Infested => 38,
            Self::Unknown(id) => id,
        }
    }

    /// Vanilla `MobEffect.getDisplayName` key under `effect.minecraft.<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Slowness => "slowness",
            Self::Haste => "haste",
            Self::MiningFatigue => "mining_fatigue",
            Self::Strength => "strength",
            Self::InstantHealth => "instant_health",
            Self::InstantDamage => "instant_damage",
            Self::JumpBoost => "jump_boost",
            Self::Nausea => "nausea",
            Self::Regeneration => "regeneration",
            Self::Resistance => "resistance",
            Self::FireResistance => "fire_resistance",
            Self::WaterBreathing => "water_breathing",
            Self::Invisibility => "invisibility",
            Self::Blindness => "blindness",
            Self::NightVision => "night_vision",
            Self::Hunger => "hunger",
            Self::Weakness => "weakness",
            Self::Poison => "poison",
            Self::Wither => "wither",
            Self::HealthBoost => "health_boost",
            Self::Absorption => "absorption",
            Self::Saturation => "saturation",
            Self::Glowing => "glowing",
            Self::Levitation => "levitation",
            Self::Luck => "luck",
            Self::SlowFalling => "slow_falling",
            Self::Darkness => "darkness",
            Self::WindCharged => "wind_charged",
            Self::Weaving => "weaving",
            Self::Oozing => "oozing",
            Self::Infested => "infested",
            Self::Unknown(_) => "unknown",
        }
    }

    /// The id of `minecraft:<name>` in the registry report, for round-trip tests.
    pub fn registry_id(self) -> Option<i32> {
        registries::id(
            "minecraft:mob_effect",
            &format!("minecraft:{}", self.name()),
        )
    }

    /// `MobEffect.isInstantenous`.
    pub fn is_instant(self) -> bool {
        matches!(self, Self::InstantHealth | Self::InstantDamage)
    }
}

/// One `MobEffectInstance`: amplifier 0 is level I, duration -1 is infinite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobEffectInstance {
    pub effect: i32,
    pub amplifier: i32,
    pub duration: i32,
    /// `ClientboundUpdateMobEffectPacket` flag 0x01.
    pub ambient: bool,
    /// Flag 0x02: show particles.
    pub visible: bool,
    /// Flag 0x04: show the effect icon.
    pub show_icon: bool,
}

impl MobEffectInstance {
    pub fn new(effect: i32, duration: i32, amplifier: i32) -> Self {
        Self {
            effect,
            amplifier,
            duration,
            ambient: false,
            visible: true,
            show_icon: true,
        }
    }

    /// An effect applied by a splash/lingering potion: ambient and stronger particles.
    pub fn ambient(effect: i32, duration: i32, amplifier: i32) -> Self {
        Self {
            ambient: true,
            ..Self::new(effect, duration, amplifier)
        }
    }

    pub fn kind(&self) -> Effect {
        Effect::from_id(self.effect)
    }

    /// The flags byte of `ClientboundUpdateMobEffectPacket`.
    pub fn flags(&self) -> u8 {
        u8::from(self.ambient) | (u8::from(self.visible) << 1) | (u8::from(self.show_icon) << 2)
    }

    pub fn is_infinite(&self) -> bool {
        self.duration == INFINITE
    }
}

/// What one effect tick asks the player to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectEvent {
    Heal(f32),
    /// `can_kill` is false for poison (`LivingEntity` stops at 1 health); `wither` selects
    /// `DamageTypes.WITHER` instead of magic.
    Damage {
        amount: f32,
        can_kill: bool,
        wither: bool,
    },
    Exhaustion(f32),
    /// `SaturationMobEffect`: +1 food and +1 saturation.
    Food,
    Absorb(f32),
    RemoveAbsorb(f32),
}

/// Result of [`ActiveEffects::tick`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectTicks {
    pub events: Vec<EffectEvent>,
    /// Effects that expired this tick (their removal packet must be sent).
    pub expired: Vec<MobEffectInstance>,
}

/// The attribute modifiers vanilla derives from active effects.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AttributeModifiers {
    /// Multiplier applied to `Attributes.MOVEMENT_SPEED` (speed +20%/level, slowness -15%).
    pub movement_speed: f64,
    /// `Attributes.ATTACK_DAMAGE` added by strength (+3/level) and weakness (-4/level).
    pub attack_damage: f64,
    /// `Attributes.MAX_HEALTH` bonus from health boost (+4/level).
    pub max_health_bonus: f32,
    /// Multiplier for `Attributes.BLOCK_BREAK_SPEED` (haste +10%, fatigue -10% per level).
    pub mining_speed: f64,
    /// Resistance: fraction of incoming damage ignored (20% per level, capped at 100%).
    pub damage_reduction: f64,
    pub jump_boost: i32,
    pub luck: f64,
}

/// `LivingEntity.activeEffects`: one instance per effect id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActiveEffects {
    instances: Vec<MobEffectInstance>,
}

impl ActiveEffects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl Iterator<Item = &MobEffectInstance> {
        self.instances.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn get(&self, effect: Effect) -> Option<&MobEffectInstance> {
        self.instances.iter().find(|i| i.effect == effect.id())
    }

    pub fn level(&self, effect: Effect) -> i32 {
        self.get(effect).map_or(0, |i| i.amplifier + 1)
    }

    pub fn has(&self, effect: Effect) -> bool {
        self.get(effect).is_some()
    }

    /// Vanilla `HealOrHarmMobEffect.applyInstantenousEffect`: healing is `(int)(scale * (4 << amp) + 0.5)`
    /// points and harming `(int)(scale * (6 << amp) + 0.5)`. Drinking is scale 1, a splash potion
    /// scales by distance and a lingering cloud applies half.
    pub fn instant_events(instance: MobEffectInstance, scale: f64) -> Vec<EffectEvent> {
        let amplifier = instance.amplifier.clamp(0, 30);
        let points = |base: i32| (scale * f64::from(base << amplifier) + 0.5) as i32 as f32;
        match instance.kind() {
            Effect::InstantHealth => vec![EffectEvent::Heal(points(4))],
            Effect::InstantDamage => vec![EffectEvent::Damage {
                amount: points(6),
                can_kill: true,
                wither: false,
            }],
            _ => Vec::new(),
        }
    }

    /// `LivingEntity.addEffect`: a weaker instance never replaces a stronger one; the same
    /// level only replaces when the new duration is longer. Instant effects apply immediately
    /// and are not stored.
    pub fn add(&mut self, instance: MobEffectInstance) -> Vec<EffectEvent> {
        let mut events = Vec::new();
        let kind = instance.kind();
        if kind.is_instant() {
            return Self::instant_events(instance, 1.0);
        }
        if let Some(old) = self.get(kind).copied() {
            let stronger = old.amplifier > instance.amplifier;
            let same_level_longer = old.amplifier == instance.amplifier
                && !instance.is_infinite()
                && (old.is_infinite() || old.duration > instance.duration);
            if stronger || same_level_longer {
                return events;
            }
            if kind == Effect::Absorption {
                events.push(EffectEvent::RemoveAbsorb(
                    4.0 * f32::from(old.amplifier as i16 + 1),
                ));
            }
        }
        if kind == Effect::Absorption {
            events.push(EffectEvent::Absorb(
                4.0 * f32::from(instance.amplifier as i16 + 1),
            ));
        }
        self.instances.retain(|i| i.effect != instance.effect);
        self.instances.push(instance);
        events
    }

    /// `LivingEntity.removeEffect`; returns the removal events (absorption cleanup).
    pub fn remove(&mut self, effect: Effect) -> Vec<EffectEvent> {
        let mut events = Vec::new();
        if let Some(at) = self.instances.iter().position(|i| i.effect == effect.id()) {
            let instance = self.instances.remove(at);
            if effect == Effect::Absorption {
                events.push(EffectEvent::RemoveAbsorb(
                    4.0 * f32::from(instance.amplifier as i16 + 1),
                ));
            }
        }
        events
    }

    /// `LivingEntity.removeAllEffects` (milk, death): returns removed instances.
    pub fn clear(&mut self) -> (Vec<MobEffectInstance>, Vec<EffectEvent>) {
        let instances = std::mem::take(&mut self.instances);
        let events = instances
            .iter()
            .filter(|i| i.effect == Effect::Absorption.id())
            .map(|i| EffectEvent::RemoveAbsorb(4.0 * f32::from(i.amplifier as i16 + 1)))
            .collect();
        (instances, events)
    }

    /// `MobEffectInstance.tick`: apply the timed effects, decrement durations and expire.
    ///
    /// Vanilla checks `duration % interval == 0` before decrementing, so a 50-tick
    /// regeneration heals on ticks 50, 100, ... in game time.
    pub fn tick(&mut self) -> EffectTicks {
        let mut events = Vec::new();
        for instance in &self.instances {
            let kind = instance.kind();
            if instance.duration <= 0 {
                continue;
            }
            match kind {
                Effect::Regeneration if instance.duration % 50 == 0 => {
                    events.push(EffectEvent::Heal(1.0));
                }
                Effect::Poison if instance.duration % 25 == 0 => {
                    events.push(EffectEvent::Damage {
                        amount: 1.0,
                        can_kill: false,
                        wither: false,
                    });
                }
                Effect::Wither if instance.duration % 40 == 0 => {
                    events.push(EffectEvent::Damage {
                        amount: 1.0,
                        can_kill: true,
                        wither: true,
                    });
                }
                Effect::Hunger => events.push(EffectEvent::Exhaustion(
                    0.005 * f32::from(instance.amplifier as i16 + 1),
                )),
                Effect::Saturation => events.push(EffectEvent::Food),
                _ => {}
            }
        }
        for instance in &mut self.instances {
            if instance.duration > 0 {
                instance.duration -= 1;
            }
        }
        let mut expired = Vec::new();
        self.instances.retain(|instance| {
            if instance.duration == 0 {
                expired.push(*instance);
                false
            } else {
                true
            }
        });
        for instance in &expired {
            if instance.effect == Effect::Absorption.id() {
                events.push(EffectEvent::RemoveAbsorb(
                    4.0 * f32::from(instance.amplifier as i16 + 1),
                ));
            }
        }
        EffectTicks { events, expired }
    }

    /// Vanilla attribute modifiers, summed per effect level.
    pub fn attributes(&self) -> AttributeModifiers {
        let speed = self.level(Effect::Speed);
        let slowness = self.level(Effect::Slowness);
        let haste = self.level(Effect::Haste);
        let fatigue = self.level(Effect::MiningFatigue);
        let strength = self.level(Effect::Strength);
        let weakness = self.level(Effect::Weakness);
        AttributeModifiers {
            movement_speed: (1.0 + 0.2 * f64::from(speed) - 0.15 * f64::from(slowness)).max(0.0),
            attack_damage: 3.0 * f64::from(strength) - 4.0 * f64::from(weakness),
            max_health_bonus: 4.0 * self.level(Effect::HealthBoost) as f32,
            mining_speed: (1.0 + 0.1 * f64::from(haste) - 0.1 * f64::from(fatigue)).max(0.0),
            damage_reduction: (0.2 * f64::from(self.level(Effect::Resistance))).min(1.0),
            jump_boost: self.level(Effect::JumpBoost),
            luck: f64::from(self.level(Effect::Luck)),
        }
    }

    /// Absorption added by the absorption effect (`4 * (amplifier + 1)`).
    pub fn absorption_bonus(&self) -> f32 {
        4.0 * self.level(Effect::Absorption) as f32
    }

    pub fn fire_resistance(&self) -> bool {
        self.has(Effect::FireResistance)
    }

    pub fn water_breathing(&self) -> bool {
        self.has(Effect::WaterBreathing)
    }

    pub fn slow_falling(&self) -> bool {
        self.has(Effect::SlowFalling)
    }

    pub fn invisibility(&self) -> bool {
        self.has(Effect::Invisibility)
    }

    pub fn night_vision(&self) -> bool {
        self.has(Effect::NightVision)
    }

    pub fn glowing(&self) -> bool {
        self.has(Effect::Glowing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_ids_match_the_registry_report() {
        for effect in Effect::ALL {
            assert_eq!(
                effect.registry_id(),
                Some(effect.id()),
                "{} id",
                effect.name()
            );
            assert_eq!(Effect::from_id(effect.id()), effect);
        }
    }

    #[test]
    fn speed_and_slowness_feed_the_movement_multiplier() {
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Speed.id(), 200, 1));
        assert!((effects.attributes().movement_speed - 1.4).abs() < 1e-9);
        effects.add(MobEffectInstance::new(Effect::Slowness.id(), 200, 0));
        assert!((effects.attributes().movement_speed - 1.25).abs() < 1e-9);
        // Slowness IV (60%) outweighs Speed II (40%).
        effects.add(MobEffectInstance::new(Effect::Slowness.id(), 200, 3));
        assert!((effects.attributes().movement_speed - 0.8).abs() < 1e-9);
    }

    #[test]
    fn strength_weakness_health_boost_and_resistance_modify_attributes() {
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Strength.id(), 200, 1));
        effects.add(MobEffectInstance::new(Effect::Weakness.id(), 200, 0));
        effects.add(MobEffectInstance::new(Effect::HealthBoost.id(), 200, 2));
        effects.add(MobEffectInstance::new(Effect::Resistance.id(), 200, 2));
        effects.add(MobEffectInstance::new(Effect::Haste.id(), 200, 1));
        let attributes = effects.attributes();
        assert!((attributes.attack_damage - 2.0).abs() < 1e-9);
        assert!((attributes.max_health_bonus - 12.0).abs() < 1e-9);
        assert!((attributes.damage_reduction - 0.6).abs() < 1e-9);
        assert!((attributes.mining_speed - 1.2).abs() < 1e-9);
    }

    #[test]
    fn regeneration_poison_and_wither_follow_vanilla_intervals() {
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Regeneration.id(), 50, 0));
        let mut healed = 0;
        for _ in 0..50 {
            let ticks = effects.tick();
            healed += ticks
                .events
                .iter()
                .filter(|e| matches!(e, EffectEvent::Heal(_)))
                .count();
        }
        assert_eq!(healed, 1);
        assert!(effects.is_empty());

        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Poison.id(), 25, 0));
        let mut poison = 0;
        for _ in 0..25 {
            poison += effects
                .tick()
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        EffectEvent::Damage {
                            can_kill: false,
                            ..
                        }
                    )
                })
                .count();
        }
        assert_eq!(poison, 1);

        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Wither.id(), 40, 0));
        let ticks = (0..40)
            .flat_map(|_| effects.tick().events)
            .collect::<Vec<_>>();
        assert!(
            ticks
                .iter()
                .any(|e| matches!(e, EffectEvent::Damage { can_kill: true, .. }))
        );
    }

    #[test]
    fn hunger_saturation_and_infinite_durations() {
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Hunger.id(), INFINITE, 1));
        assert!(
            !effects
                .tick()
                .events
                .iter()
                .any(|e| matches!(e, EffectEvent::Exhaustion(_))),
            "vanilla only ticks lasting effects"
        );
        assert_eq!(effects.len(), 1);
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Saturation.id(), 20, 0));
        assert_eq!(effects.tick().events, [EffectEvent::Food]);
    }

    #[test]
    fn absorption_adds_and_removes_its_shield() {
        let mut effects = ActiveEffects::default();
        assert_eq!(
            effects.add(MobEffectInstance::new(Effect::Absorption.id(), 200, 1)),
            [EffectEvent::Absorb(8.0)]
        );
        assert_eq!(effects.absorption_bonus(), 8.0);
        assert_eq!(
            effects.remove(Effect::Absorption),
            [EffectEvent::RemoveAbsorb(8.0)]
        );
        assert!(!effects.has(Effect::Absorption));
    }

    #[test]
    fn instant_effects_apply_once_and_are_not_stored() {
        let mut effects = ActiveEffects::default();
        assert_eq!(
            effects.add(MobEffectInstance::new(Effect::InstantHealth.id(), 1, 1)),
            [EffectEvent::Heal(8.0)]
        );
        assert!(effects.is_empty());
        assert!(matches!(
            effects
                .add(MobEffectInstance::new(Effect::InstantDamage.id(), 1, 0))
                .as_slice(),
            [EffectEvent::Damage {
                amount: 6.0,
                can_kill: true,
                wither: false
            }]
        ));
    }

    #[test]
    fn stronger_and_longer_instances_win_and_clear_removes_everything() {
        let mut effects = ActiveEffects::default();
        effects.add(MobEffectInstance::new(Effect::Speed.id(), 100, 1));
        assert!(
            effects
                .add(MobEffectInstance::new(Effect::Speed.id(), 200, 0))
                .is_empty()
        );
        assert_eq!(effects.level(Effect::Speed), 2);
        assert!(
            effects
                .add(MobEffectInstance::new(Effect::Speed.id(), 50, 1))
                .is_empty()
        );
        assert_eq!(effects.get(Effect::Speed).unwrap().duration, 100);
        effects.add(MobEffectInstance::new(Effect::Speed.id(), 300, 1));
        assert_eq!(effects.get(Effect::Speed).unwrap().duration, 300);
        let (removed, _) = effects.clear();
        assert_eq!(removed.len(), 1);
        assert!(effects.is_empty());
    }

    #[test]
    fn update_packet_flags_cover_ambient_particles_and_icon() {
        let mut instance = MobEffectInstance::new(Effect::Speed.id(), 100, 0);
        assert_eq!(instance.flags(), 0b110);
        instance.ambient = true;
        instance.show_icon = false;
        assert_eq!(instance.flags(), 0b011);
    }
}
