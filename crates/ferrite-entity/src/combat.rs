//! Vanilla player combat maths: weapon attack damage and speed from the item's
//! `minecraft:attribute_modifiers` component, the attack-strength scale, critical hits,
//! sweeping and entity reach. Nothing here touches the world; the region applies these to the
//! lid of an entity.
//!
//! The constants mirror `Player`: base attack damage 1.0, base attack speed 4.0, the entity
//! interaction range of 3 blocks (5 in creative), `Player.attack`'s critical multiplier 1.5 and
//! `Player.ATTACK_STRENGTH_DECAY`-free scale `0.2 + strength^2 * 0.8`.

use ferrite_item::{AttributeOperation, Component, component_ids};
use ferrite_math::{AABB, Vec3};

/// Vanilla `Player` base attack damage attribute (`ATTACK_DAMAGE`, base value 1.0).
pub const BASE_ATTACK_DAMAGE: f64 = 1.0;
/// Vanilla `Player` base attack speed attribute (`ATTACK_SPEED`, base value 4.0).
pub const BASE_ATTACK_SPEED: f64 = 4.0;
/// Vanilla `Player.entityInteractionRange()` in survival.
pub const ENTITY_RANGE: f64 = 3.0;
/// Vanilla creative-mode entity reach (`ServerPlayer` uses 5 blocks).
pub const CREATIVE_ENTITY_RANGE: f64 = 5.0;
/// `Player.attack`: a critical hit multiplies by 1.5 (`ATTACK_DAMAGE` is already scaled).
pub const CRITICAL_MULTIPLIER: f64 = 1.5;
/// `Player.attack`: sweep damage is `1.0 + sweeping ratio * damage`, without enchantments 1.
pub const SWEEP_DAMAGE: f32 = 1.0;
/// Vanilla `AbstractArrow` base damage before the velocity scale.
pub const ARROW_BASE_DAMAGE: f32 = 2.0;

/// The attack damage and speed a held stack grants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponStats {
    /// Total attack damage, including the player's base 1.0.
    pub attack_damage: f64,
    /// Attack speed in attacks per second (vanilla's `ATTACK_SPEED`).
    pub attack_speed: f64,
}

/// Applies one attribute modifier to a base value, like `AttributeInstance`.
fn apply(base: f64, value: &mut f64, operation: AttributeOperation, amount: f64) {
    match operation {
        AttributeOperation::AddValue => *value += amount,
        AttributeOperation::AddMultipliedBase => *value += base * amount,
        AttributeOperation::AddMultipliedTotal => *value *= 1.0 + amount,
    }
}

/// Attack statistics of a held stack: the item's attribute modifiers over the player's base
/// attributes. Items only modify the main hand; an off-hand or armor modifier is ignored.
pub fn weapon_stats(stack: &ferrite_item::ItemStack) -> WeaponStats {
    let mut attack_damage = BASE_ATTACK_DAMAGE;
    let mut attack_speed = BASE_ATTACK_SPEED;
    if let Some(Component::AttributeModifiers(modifiers)) =
        stack.effective_component(component_ids::ATTRIBUTE_MODIFIERS)
    {
        for modifier in modifiers {
            if !matches!(modifier.slot.name(), "mainhand" | "hand" | "any") {
                continue;
            }
            let attribute = modifier
                .attribute
                .strip_prefix("minecraft:")
                .unwrap_or(&modifier.attribute);
            match attribute {
                "attack_damage" => apply(
                    BASE_ATTACK_DAMAGE,
                    &mut attack_damage,
                    modifier.operation,
                    modifier.amount,
                ),
                "attack_speed" => apply(
                    BASE_ATTACK_SPEED,
                    &mut attack_speed,
                    modifier.operation,
                    modifier.amount,
                ),
                _ => {}
            }
        }
    }
    WeaponStats {
        attack_damage,
        attack_speed: attack_speed.max(0.1),
    }
}

/// Vanilla `Player.getAttackStrengthScale(0.5)`: `(ticks + 0.5) / (20 / attackSpeed)`, clamped.
pub fn attack_strength(ticks_since_attack: i32, attack_speed: f64) -> f64 {
    let delay = 20.0 / attack_speed.max(0.1);
    ((f64::from(ticks_since_attack) + 0.5) / delay).clamp(0.0, 1.0)
}

/// Vanilla's damage scale from the attack strength: `0.2 + strength^2 * 0.8`.
pub fn damage_scale(strength: f64) -> f64 {
    0.2 + strength.clamp(0.0, 1.0).powi(2) * 0.8
}

/// Vanilla `Player.attack` critical condition: falling, not on the ground, not sprinting.
/// Climbing, water and blindness are not modelled.
pub fn can_critical(on_ground: bool, fall_distance: f64, sprinting: bool) -> bool {
    !on_ground && fall_distance > 0.0 && !sprinting
}

/// Vanilla sweep condition: a sword, a nearly full attack strength, on the ground and not
/// sprinting.
pub fn can_sweep(sword: bool, strength: f64, on_ground: bool, sprinting: bool) -> bool {
    sword && strength > 0.9 && on_ground && !sprinting
}

/// Whether an item is one of the sword items, which are the only ones that sweep.
pub fn is_sword(stack: &ferrite_item::ItemStack) -> bool {
    stack.name().is_some_and(|name| name.ends_with("_sword"))
}

/// Mobs one spear thrust can hit (`spear_mobs` asks for five).
pub const SPEAR_MAX_TARGETS: usize = 5;

/// A weapon's `minecraft:attack_range` component (26.3 `AttackRange`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackRange {
    pub min_reach: f64,
    pub max_reach: f64,
    pub min_creative_reach: f64,
    pub max_creative_reach: f64,
    /// The target's box is widened by this much before the reach test.
    pub hitbox_margin: f64,
    pub mob_factor: f64,
}

/// The spear's `minecraft:attack_range`, from the 26.3 `iron_spear`/`diamond_spear` reports.
pub const SPEAR_ATTACK_RANGE: AttackRange = AttackRange {
    min_reach: 2.0,
    max_reach: 4.5,
    min_creative_reach: 2.0,
    max_creative_reach: 6.5,
    hitbox_margin: 0.125,
    mob_factor: 0.5,
};

/// `minecraft:minimum_attack_charge` of a spear: a thrust only lands at full charge.
pub const SPEAR_MINIMUM_CHARGE: f64 = 1.0;

/// The `minecraft:attack_range` of `stack`, for the weapons this server models (spears).
pub fn attack_range(stack: &ferrite_item::ItemStack) -> Option<AttackRange> {
    is_spear(stack).then_some(SPEAR_ATTACK_RANGE)
}

/// `minecraft:minimum_attack_charge`; zero means every swing lands.
pub fn minimum_attack_charge(stack: &ferrite_item::ItemStack) -> f64 {
    if is_spear(stack) {
        SPEAR_MINIMUM_CHARGE
    } else {
        0.0
    }
}

/// [`within_reach`] with the weapon's `hitbox_margin` around the target box.
pub fn within_reach_margin(eye: Vec3, target: AABB, range: f64, margin: f64) -> bool {
    let target = AABB::new(
        target.min.x - margin,
        target.min.y - margin,
        target.min.z - margin,
        target.max.x + margin,
        target.max.y + margin,
        target.max.z + margin,
    );
    within_reach(eye, target, range)
}

/// Whether the look ray hits `target`, widened by `margin`, within `range` blocks. This is the
/// pick shape a spear thrust pierces along (`EntityHitResult` with `AttackRange.hitboxMargin`).
pub fn ray_hits(eye: Vec3, look: Vec3, target: AABB, range: f64, margin: f64) -> bool {
    let origin = [eye.x, eye.y, eye.z];
    let direction = [look.x, look.y, look.z];
    let low = [
        target.min.x - margin,
        target.min.y - margin,
        target.min.z - margin,
    ];
    let high = [
        target.max.x + margin,
        target.max.y + margin,
        target.max.z + margin,
    ];
    let (mut near, mut far) = (0.0f64, range.max(0.0));
    for axis in 0..3 {
        if direction[axis].abs() < 1.0e-9 {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return false;
            }
            continue;
        }
        let inverse = 1.0 / direction[axis];
        let mut first = (low[axis] - origin[axis]) * inverse;
        let mut second = (high[axis] - origin[axis]) * inverse;
        if first > second {
            std::mem::swap(&mut first, &mut second);
        }
        near = near.max(first);
        far = far.min(second);
        if near > far {
            return false;
        }
    }
    true
}

/// Whether an item is a spear (`#minecraft:spears`), whose thrust pierces a line of mobs.
pub fn is_spear(stack: &ferrite_item::ItemStack) -> bool {
    stack.name().is_some_and(|name| {
        name.ends_with("_spear") || ferrite_data::tags::contains("item", "spears", name)
    })
}

/// Vanilla `Entity.getViewVector`: the unit look direction from yaw and pitch in degrees.
pub fn look_vector(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
    let (sp, cp) = f64::from(pitch).to_radians().sin_cos();
    Vec3::new(-sy * cp, -sp, cy * cp)
}

/// The eye position of a player standing at `feet`.
pub fn eye_position(feet: Vec3) -> Vec3 {
    Vec3::new(feet.x, feet.y + 1.62, feet.z)
}

/// Vanilla `Player.canHitEntity`: the target's box within reach of the eye point.
pub fn within_reach(eye: Vec3, target: AABB, range: f64) -> bool {
    let closest = Vec3::new(
        eye.x.clamp(target.min.x, target.max.x),
        eye.y.clamp(target.min.y, target.max.y),
        eye.z.clamp(target.min.z, target.max.z),
    );
    (closest - eye).length_sqr() <= range * range
}

/// Whether a target is in front of the look vector, so a swing behind the player misses.
pub fn facing(eye: Vec3, look: Vec3, target: Vec3) -> bool {
    let to = target - eye;
    let horizontal = Vec3::new(to.x, 0.0, to.z);
    let facing = Vec3::new(look.x, 0.0, look.z);
    if horizontal.length_sqr() < 1.0e-6 || facing.length_sqr() < 1.0e-6 {
        return true;
    }
    facing.normalize().dot(horizontal.normalize()) > -0.1
}

/// Vanilla `LivingEntity.isDamageSourceBlocked`: a raised shield stops a hit coming from the
/// half-space in front of the player's body yaw.
pub fn shield_blocks(yaw: f32, player: Vec3, source: Vec3) -> bool {
    let look = look_vector(yaw, 0.0);
    let to = source - player;
    let horizontal = Vec3::new(to.x, 0.0, to.z);
    if horizontal.length_sqr() < 1.0e-6 {
        return true;
    }
    look.normalize().dot(horizontal.normalize()) > 0.0
}

/// Vanilla `AbstractArrow` damage: base damage scaled by the draw (speed / 3), critical arrows
/// hit for 1.5x. A full-draw arrow does 6 damage.
pub fn arrow_damage(draw: f32, critical: bool) -> f32 {
    arrow_damage_with_power(draw, critical, 0)
}

/// Vanilla `AbstractArrow` damage with the Bow `minecraft:power` enchantment.
/// Power adds `0.5 * level + 0.5` to the base arrow damage before draw scaling.
pub fn arrow_damage_with_power(draw: f32, critical: bool, power_level: i32) -> f32 {
    let base_damage = if power_level > 0 {
        ARROW_BASE_DAMAGE + 0.5 * power_level as f32 + 0.5
    } else {
        ARROW_BASE_DAMAGE
    };
    let base = base_damage * (1.0 + 2.0 * draw.clamp(0.0, 1.0));
    if critical { base * 1.5 } else { base }
}

/// Reads an enchantment level from `stack`'s `minecraft:enchantments` component.
/// Accepts either `"minecraft:sharpness"` or `"sharpness"`.
pub fn enchantment_level(stack: &ferrite_item::ItemStack, name: &str) -> i32 {
    let Some(Component::Enchantments(ench)) =
        stack.effective_component(component_ids::ENCHANTMENTS)
    else {
        return 0;
    };
    if let Some(level) = ench.level(name) {
        return level.max(0);
    }
    if let Some(short) = name.strip_prefix("minecraft:") {
        ench.level(short).unwrap_or(0).max(0)
    } else {
        let full = format!("minecraft:{name}");
        ench.level(&full).unwrap_or(0).max(0)
    }
}

/// Vanilla melee enchantment damage bonus (`Sharpness`, `Smite`, `Bane of Arthropods`, and
/// Mace smash + `Density`). Added after attack-strength scaling (`Player.attack`).
pub fn melee_enchantment_bonus(
    stack: &ferrite_item::ItemStack,
    is_undead: bool,
    is_arthropod: bool,
    fall_distance: f64,
) -> f32 {
    let mut bonus = 0.0f32;
    let sharp = enchantment_level(stack, "minecraft:sharpness");
    if sharp > 0 {
        bonus += 0.5 * sharp as f32 + 0.5;
    }
    if is_undead {
        let smite = enchantment_level(stack, "minecraft:smite");
        if smite > 0 {
            bonus += 2.5 * smite as f32;
        }
    }
    if is_arthropod {
        let bane = enchantment_level(stack, "minecraft:bane_of_arthropods");
        if bane > 0 {
            bonus += 2.5 * bane as f32;
        }
    }
    if stack.name() == Some("minecraft:mace") && fall_distance > 1.5 {
        let d = fall_distance as f32;
        let smash = if d <= 3.0 {
            4.0 * d
        } else if d <= 8.0 {
            12.0 + 2.0 * (d - 3.0)
        } else {
            22.0 + (d - 8.0)
        };
        let density = enchantment_level(stack, "minecraft:density");
        bonus += smash + 0.5 * density as f32 * d;
    }
    bonus
}

/// Vanilla `Player.attack` sweeping damage with `minecraft:sweeping_edge`:
/// `1.0 + (level / (level + 1)) * weapon_damage`, or `1.0` without Sweeping Edge.
pub fn sweeping_damage(weapon_damage: f32, sweeping_level: i32) -> f32 {
    if sweeping_level <= 0 {
        SWEEP_DAMAGE
    } else {
        let ratio = sweeping_level as f32 / (sweeping_level as f32 + 1.0);
        SWEEP_DAMAGE + ratio * weapon_damage.max(0.0)
    }
}

/// Combined defense, toughness, protection enchantment EPFs, and Thorns across worn armor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmorStats {
    pub defense: f32,
    pub toughness: f32,
    pub epf_generic: i32,
    pub epf_fire: i32,
    pub epf_blast: i32,
    pub epf_projectile: i32,
    pub epf_fall: i32,
    pub thorns_total: i32,
}

/// Computes vanilla armor defense, toughness, protection EPFs, and Thorns across the four
/// worn armor stacks (`FEET..=HEAD`).
pub fn armor_stats(armor_slots: &[&ferrite_item::ItemStack]) -> ArmorStats {
    let mut stats = ArmorStats::default();
    for stack in armor_slots {
        if stack.is_empty() {
            continue;
        }
        if let Some(Component::AttributeModifiers(modifiers)) =
            stack.effective_component(component_ids::ATTRIBUTE_MODIFIERS)
        {
            for modifier in modifiers {
                if !matches!(
                    modifier.slot.name(),
                    "feet" | "legs" | "chest" | "head" | "armor" | "any"
                ) {
                    continue;
                }
                let attribute = modifier
                    .attribute
                    .strip_prefix("minecraft:")
                    .unwrap_or(&modifier.attribute);
                match attribute {
                    "armor" => {
                        let mut v = f64::from(stats.defense);
                        apply(0.0, &mut v, modifier.operation, modifier.amount);
                        stats.defense = v as f32;
                    }
                    "armor_toughness" => {
                        let mut v = f64::from(stats.toughness);
                        apply(0.0, &mut v, modifier.operation, modifier.amount);
                        stats.toughness = v as f32;
                    }
                    _ => {}
                }
            }
        }
        stats.epf_generic += enchantment_level(stack, "minecraft:protection");
        stats.epf_fire += enchantment_level(stack, "minecraft:fire_protection") * 2;
        stats.epf_blast += enchantment_level(stack, "minecraft:blast_protection") * 2;
        stats.epf_projectile += enchantment_level(stack, "minecraft:projectile_protection") * 2;
        stats.epf_fall += enchantment_level(stack, "minecraft:feather_falling") * 3;
        stats.thorns_total += enchantment_level(stack, "minecraft:thorns");
    }
    stats
}

/// Vanilla `CombatRules.getDamageAfterAbsorb`: armor defense + toughness damage reduction.
pub fn damage_after_armor(damage: f32, defense: f32, toughness: f32) -> f32 {
    if defense <= 0.0 || damage <= 0.0 {
        return damage.max(0.0);
    }
    let f = 2.0 + toughness.max(0.0) / 4.0;
    let reduction = (defense - damage / f).max(defense * 0.2).clamp(0.0, 20.0);
    damage * (1.0 - reduction / 25.0)
}

/// Vanilla `CombatRules.getDamageAfterMagicAbsorb`: protection EPF reduction (capped at 20 = 80%).
pub fn damage_after_protection(damage: f32, epf: i32) -> f32 {
    if epf <= 0 || damage <= 0.0 {
        return damage.max(0.0);
    }
    let capped = epf.clamp(0, 20) as f32;
    damage * (1.0 - capped / 25.0)
}

/// Vanilla mining speed multiplier from `minecraft:efficiency` (`level^2 + 1` bonus speed).
pub fn mining_speed_multiplier(stack: &ferrite_item::ItemStack) -> f64 {
    let eff = enchantment_level(stack, "minecraft:efficiency");
    let base = match stack.name() {
        Some(n) if n.starts_with("minecraft:netherite_") => 9.0,
        Some(n) if n.starts_with("minecraft:diamond_") => 8.0,
        Some(n) if n.starts_with("minecraft:iron_") => 6.0,
        Some(n) if n.starts_with("minecraft:stone_") => 4.0,
        Some(n) if n.starts_with("minecraft:golden_") => 12.0,
        Some(n) if n.starts_with("minecraft:wooden_") => 2.0,
        _ => 1.0,
    };
    if eff > 0 {
        base + f64::from(eff * eff + 1)
    } else {
        base
    }
}

/// Combined `minecraft:respiration` level across worn armor (extends underwater air supply).
pub fn respiration_level(armor_slots: &[&ferrite_item::ItemStack]) -> i32 {
    armor_slots
        .iter()
        .map(|s| enchantment_level(s, "minecraft:respiration"))
        .max()
        .unwrap_or(0)
        .clamp(0, 3)
}

/// `minecraft:depth_strider` level on worn boots (0..=3, scales underwater movement speed).
pub fn depth_strider_level(armor_slots: &[&ferrite_item::ItemStack]) -> i32 {
    armor_slots
        .iter()
        .map(|s| enchantment_level(s, "minecraft:depth_strider"))
        .max()
        .unwrap_or(0)
        .clamp(0, 3)
}

/// `minecraft:frost_walker` level on worn boots (0..=2, freezes surface water within `2 + lvl`).
pub fn frost_walker_level(armor_slots: &[&ferrite_item::ItemStack]) -> i32 {
    armor_slots
        .iter()
        .map(|s| enchantment_level(s, "minecraft:frost_walker"))
        .max()
        .unwrap_or(0)
        .clamp(0, 2)
}

/// `minecraft:swift_sneak` level on worn leggings (0..=3, increases sneaking speed).
pub fn swift_sneak_level(armor_slots: &[&ferrite_item::ItemStack]) -> i32 {
    armor_slots
        .iter()
        .map(|s| enchantment_level(s, "minecraft:swift_sneak"))
        .max()
        .unwrap_or(0)
        .clamp(0, 3)
}

/// Whether `stack` carries `minecraft:binding_curse`.
pub fn has_binding_curse(stack: &ferrite_item::ItemStack) -> bool {
    enchantment_level(stack, "minecraft:binding_curse") > 0
}

/// Vanilla `minecraft:impaling` bonus damage (`2.5 * level`) against aquatic or wet targets.
pub fn impaling_bonus(stack: &ferrite_item::ItemStack, aquatic_or_wet: bool) -> f32 {
    if !aquatic_or_wet {
        return 0.0;
    }
    let lvl = enchantment_level(stack, "minecraft:impaling");
    if lvl > 0 { 2.5 * lvl as f32 } else { 0.0 }
}

/// Crossbow charge time in ticks (`25 - 5 * quick_charge`, min 5).
pub fn crossbow_charge_ticks(stack: &ferrite_item::ItemStack) -> i32 {
    let qc = enchantment_level(stack, "minecraft:quick_charge").clamp(0, 4);
    (25 - 5 * qc).max(5)
}

/// Whether a crossbow has `minecraft:multishot`.
pub fn multishot_enabled(stack: &ferrite_item::ItemStack) -> bool {
    enchantment_level(stack, "minecraft:multishot") > 0
}

/// `minecraft:piercing` level on a crossbow.
pub fn piercing_level(stack: &ferrite_item::ItemStack) -> i32 {
    enchantment_level(stack, "minecraft:piercing").clamp(0, 4)
}

/// `minecraft:wind_burst` level on a mace (upward burst on smash hit).
pub fn wind_burst_level(stack: &ferrite_item::ItemStack) -> i32 {
    enchantment_level(stack, "minecraft:wind_burst").clamp(0, 3)
}

/// 26.3 `minecraft:lunge` level on a spear/weapon (forward dash impulse on attack).
pub fn lunge_level(stack: &ferrite_item::ItemStack) -> i32 {
    enchantment_level(stack, "minecraft:lunge").clamp(0, 3)
}

/// Whether any worn armor piece is leather (`#minecraft:freeze_immune_wearables`).
pub fn is_wearing_leather_armor(armor_slots: &[&ferrite_item::ItemStack]) -> bool {
    armor_slots.iter().any(|s| {
        s.name()
            .is_some_and(|n| n.starts_with("minecraft:leather_"))
    })
}

/// One attribute of a single armor stack for the given equipment slot, like
/// `Mob.getApproximateAttributeWith` (the item's own base is zero).
fn armor_attribute(
    stack: &ferrite_item::ItemStack,
    attribute: &str,
    slot: &str,
    slots: &[&str],
) -> f64 {
    let mut value = 0.0;
    if let Some(Component::AttributeModifiers(modifiers)) =
        stack.effective_component(component_ids::ATTRIBUTE_MODIFIERS)
    {
        for modifier in modifiers {
            let modifier_slot = modifier.slot.name();
            if modifier_slot != slot && !slots.contains(&modifier_slot) {
                continue;
            }
            let name = modifier
                .attribute
                .strip_prefix("minecraft:")
                .unwrap_or(&modifier.attribute);
            if name == attribute {
                apply(0.0, &mut value, modifier.operation, modifier.amount);
            }
        }
    }
    value
}

/// Vanilla `Mob.compareArmor`: the armor points of one stack in `slot` (`"head"`, `"chest"`,
/// `"legs"` or `"feet"`).
pub fn armor_points(stack: &ferrite_item::ItemStack, slot: &str) -> f64 {
    armor_attribute(stack, "armor", slot, &["armor", "any"])
}

/// The armor toughness of one stack in `slot`.
pub fn armor_toughness(stack: &ferrite_item::ItemStack, slot: &str) -> f64 {
    armor_attribute(stack, "armor_toughness", slot, &["armor", "any"])
}

/// Number of enchantments on a stack, for `Mob.canReplaceEqualItem`'s first tie-break.
pub fn enchantment_count(stack: &ferrite_item::ItemStack) -> usize {
    match stack.effective_component(component_ids::ENCHANTMENTS) {
        Some(Component::Enchantments(ench)) => ench.levels.len(),
        _ => 0,
    }
}

/// The `minecraft:damage` value of a stack (0 when undamaged).
pub fn damage_value(stack: &ferrite_item::ItemStack) -> i32 {
    match stack.effective_component(component_ids::DAMAGE) {
        Some(Component::Damage(value)) => value,
        _ => 0,
    }
}

/// Whether a stack carries a `minecraft:custom_name`.
pub fn has_custom_name(stack: &ferrite_item::ItemStack) -> bool {
    stack
        .effective_component(component_ids::CUSTOM_NAME)
        .is_some()
}

/// Vanilla `Mob.canReplaceCurrentItem` for armor: better armor points win, then toughness,
/// then more enchantments, then less damage, then a custom name.
pub fn better_armor(
    candidate: &ferrite_item::ItemStack,
    current: &ferrite_item::ItemStack,
    slot: &str,
) -> bool {
    // `Mob.compareArmor`: a worn piece with the `prevent_armor_change` effect (binding curse)
    // is never replaced.
    if enchantment_level(current, "minecraft:binding_curse") > 0 {
        return false;
    }
    let candidate_points = armor_points(candidate, slot);
    let current_points = armor_points(current, slot);
    if candidate_points != current_points {
        return candidate_points > current_points;
    }
    let candidate_toughness = armor_toughness(candidate, slot);
    let current_toughness = armor_toughness(current, slot);
    if candidate_toughness != current_toughness {
        return candidate_toughness > current_toughness;
    }
    let candidate_enchants = enchantment_count(candidate);
    let current_enchants = enchantment_count(current);
    if candidate_enchants != current_enchants {
        return candidate_enchants > current_enchants;
    }
    let candidate_damage = damage_value(candidate);
    let current_damage = damage_value(current);
    if candidate_damage != current_damage {
        return candidate_damage < current_damage;
    }
    has_custom_name(candidate) && !has_custom_name(current)
}

/// A `KineticWeapon$Condition`: all three checks must pass, scaled by the attacker factor
/// (1.0 for players, 0.2 for mobs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticCondition {
    pub max_duration_ticks: i32,
    pub min_speed: f64,
    pub min_relative_speed: f64,
}

impl KineticCondition {
    /// Vanilla `Condition.test(duration, speed, relativeSpeed, factor)`.
    pub fn test(&self, duration_ticks: i32, speed: f64, relative_speed: f64, factor: f64) -> bool {
        duration_ticks <= self.max_duration_ticks
            && speed >= self.min_speed * factor
            && relative_speed >= self.min_relative_speed * factor
    }
}

/// The `minecraft:kinetic_weapon` component of a spear (26.3 `iron_spear` report).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticWeapon {
    /// Ticks before the same body can be stabbed again (`contact_cooldown_ticks`, default 10).
    pub contact_cooldown_ticks: i32,
    /// Ticks the spear must be used before it can hit (`delay_ticks`).
    pub delay_ticks: i32,
    pub dismount: KineticCondition,
    pub knockback: KineticCondition,
    pub damage: KineticCondition,
    /// `forward_movement` from the component. 26.3 reads it only in the codec: no server-side
    /// `KineticWeapon` code applies it, so stabs do not push the attacker along.
    pub forward_movement: f64,
    /// `damage = ATTACK_DAMAGE base + floor(relative_speed * damage_multiplier)`.
    pub damage_multiplier: f64,
}

/// Every spear carries this component in 26.3 (the reports are identical across materials).
pub const SPEAR_KINETIC_WEAPON: KineticWeapon = KineticWeapon {
    contact_cooldown_ticks: 10,
    delay_ticks: 12,
    dismount: KineticCondition {
        max_duration_ticks: 50,
        min_speed: 11.0,
        min_relative_speed: 0.0,
    },
    knockback: KineticCondition {
        max_duration_ticks: 135,
        min_speed: 5.1,
        min_relative_speed: 0.0,
    },
    damage: KineticCondition {
        max_duration_ticks: 225,
        min_speed: 0.0,
        min_relative_speed: 4.6,
    },
    forward_movement: 0.38,
    damage_multiplier: 0.95,
};

/// `KineticWeapon.computeDamageUseDuration`: delay plus the damage window, the 237-tick use.
pub const SPEAR_USE_DURATION: i32 =
    SPEAR_KINETIC_WEAPON.delay_ticks + SPEAR_KINETIC_WEAPON.damage.max_duration_ticks;

/// The `minecraft:kinetic_weapon` of `stack`, for the weapons this server models.
pub fn kinetic_weapon(stack: &ferrite_item::ItemStack) -> Option<KineticWeapon> {
    is_spear(stack).then_some(SPEAR_KINETIC_WEAPON)
}

/// Vanilla `KineticWeapon.damageEntities` damage: the player's base attack damage (1.0) plus
/// `floor(relative_speed * damage_multiplier)`.
pub fn kinetic_damage(attack_damage_base: f64, relative_speed: f64) -> f32 {
    (attack_damage_base + (relative_speed * SPEAR_KINETIC_WEAPON.damage_multiplier).floor()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str) -> ferrite_item::ItemStack {
        ferrite_item::ItemStack::from_name(name).expect("a vanilla item")
    }

    #[test]
    fn swords_beat_fists_and_faster_weapons_charge_faster() {
        let sword = weapon_stats(&stack("minecraft:diamond_sword"));
        assert!(sword.attack_damage > BASE_ATTACK_DAMAGE, "{sword:?}");
        assert!(sword.attack_damage > weapon_stats(&stack("minecraft:wooden_sword")).attack_damage);
        assert_eq!(sword.attack_damage, 7.0);
        assert!((sword.attack_speed - 1.6).abs() < 1e-6, "{sword:?}");
        let axe = weapon_stats(&stack("minecraft:diamond_axe"));
        assert!(axe.attack_speed < sword.attack_speed, "{axe:?}");
        assert!(axe.attack_damage > sword.attack_damage, "{axe:?}");
        // An empty hand has the player's base attributes.
        assert_eq!(
            weapon_stats(&ferrite_item::ItemStack::default()),
            WeaponStats {
                attack_damage: BASE_ATTACK_DAMAGE,
                attack_speed: BASE_ATTACK_SPEED
            }
        );
    }

    #[test]
    fn attack_strength_scales_damage_from_a_fifth_to_full() {
        assert!((attack_strength(0, 4.0) - 0.1).abs() < 1e-9);
        assert!((attack_strength(4, 4.0) - 0.9).abs() < 1e-9);
        assert_eq!(attack_strength(100, 4.0), 1.0);
        assert!((damage_scale(0.0) - 0.2).abs() < 1e-9);
        assert!((damage_scale(1.0) - 1.0).abs() < 1e-9);
        assert!(damage_scale(0.5) < damage_scale(1.0));
        // A slower weapon needs more ticks for the same strength.
        assert!(attack_strength(10, 1.6) < attack_strength(10, 4.0));
    }

    #[test]
    fn criticals_and_sweeps_follow_the_vanilla_conditions() {
        assert!(can_critical(false, 1.5, false));
        assert!(!can_critical(true, 1.5, false));
        assert!(!can_critical(false, 0.0, false));
        assert!(!can_critical(false, 1.5, true));
        assert!(can_sweep(true, 0.95, true, false));
        assert!(!can_sweep(false, 0.95, true, false));
        assert!(!can_sweep(true, 0.5, true, false));
        assert!(!can_sweep(true, 0.95, false, false));
        assert!(!can_sweep(true, 0.95, true, true));
        assert!(is_sword(&stack("minecraft:iron_sword")));
        assert!(!is_sword(&stack("minecraft:iron_axe")));
        assert!(is_spear(&stack("minecraft:iron_spear")));
        assert!(!is_spear(&stack("minecraft:iron_sword")));
    }

    #[test]
    fn reach_facing_and_look_vectors_match_the_client_conventions() {
        let eye = Vec3::new(0.5, 1.62, 0.5);
        let target = AABB::new(2.0, 0.0, 0.0, 3.0, 1.8, 1.0);
        assert!(within_reach(eye, target, ENTITY_RANGE));
        assert!(!within_reach(eye, target, 1.0));
        // Yaw zero looks south (+z) and yaw -90 looks east (+x).
        let south = look_vector(0.0, 0.0);
        assert!((south.z - 1.0).abs() < 1e-9);
        let east = look_vector(-90.0, 0.0);
        assert!((east.x - 1.0).abs() < 1e-9);
        assert!(facing(eye, east, Vec3::new(3.0, 0.0, 0.5)));
        assert!(!facing(eye, east, Vec3::new(-3.0, 0.0, 0.5)));
        // Looking down pitches negative.
        assert!(look_vector(0.0, 60.0).y < 0.0);
    }

    #[test]
    fn a_spear_reaches_further_and_pierces_along_its_ray() {
        let spear = stack("minecraft:iron_spear");
        let reach = attack_range(&spear).expect("a spear");
        assert_eq!(reach.max_reach, 4.5);
        assert_eq!(reach.max_creative_reach, 6.5);
        assert_eq!(reach.hitbox_margin, 0.125);
        assert_eq!(minimum_attack_charge(&spear), 1.0);
        assert_eq!(
            minimum_attack_charge(&stack("minecraft:diamond_sword")),
            0.0
        );
        assert!(attack_range(&stack("minecraft:diamond_sword")).is_none());

        let eye = Vec3::new(0.0, 1.62, 0.0);
        let look = Vec3::new(0.0, 0.0, 1.0);
        let near = AABB::new(-0.3, 0.0, 3.0, 0.3, 1.8, 3.6);
        let far = AABB::new(-0.3, 0.0, 5.0, 0.3, 1.8, 5.6);
        assert!(ray_hits(
            eye,
            look,
            near,
            reach.max_reach,
            reach.hitbox_margin
        ));
        assert!(!ray_hits(
            eye,
            look,
            far,
            reach.max_reach,
            reach.hitbox_margin
        ));
        // A box just off the ray only counts once the hitbox margin widens it.
        let edge = AABB::new(0.05, 0.0, 2.0, 0.3, 1.8, 2.6);
        assert!(!ray_hits(eye, look, edge, reach.max_reach, 0.0));
        assert!(ray_hits(
            eye,
            look,
            edge,
            reach.max_reach,
            reach.hitbox_margin
        ));
        // The margin also extends the reach past the box.
        let behind = AABB::new(-0.3, 0.0, -3.0, 0.3, 1.8, -2.4);
        assert!(!ray_hits(
            eye,
            look,
            behind,
            reach.max_reach,
            reach.hitbox_margin
        ));
    }

    #[test]
    fn kinetics_and_armor_upgrades_follow_the_26_3_components() {
        let spear = stack("minecraft:iron_spear");
        let kinetic = kinetic_weapon(&spear).expect("a spear");
        assert_eq!(kinetic.delay_ticks, 12);
        assert_eq!(SPEAR_USE_DURATION, 237);
        // Damage needs the delayed window and 4.6 blocks/s of relative speed.
        assert!(!kinetic.damage.test(12 + 225 + 1, 10.0, 10.0, 1.0));
        assert!(!kinetic.damage.test(20, 10.0, 4.59, 1.0));
        assert!(kinetic.damage.test(20, 10.0, 4.6, 1.0));
        // Knockback and dismount only look at the attacker's own speed.
        assert!(!kinetic.knockback.test(20, 5.09, 0.0, 1.0));
        assert!(kinetic.knockback.test(20, 5.1, 0.0, 1.0));
        assert!(!kinetic.knockback.test(136, 20.0, 0.0, 1.0));
        assert!(kinetic.dismount.test(50, 11.0, 0.0, 1.0));
        assert!(!kinetic.dismount.test(51, 11.0, 0.0, 1.0));
        // Damage ignores the weapon's own attack damage and uses the base 1.0.
        assert_eq!(kinetic_damage(1.0, 13.7), 14.0);
        assert_eq!(kinetic_damage(1.0, 4.6), 5.0);
        assert_eq!(kinetic_weapon(&stack("minecraft:diamond_sword")), None);

        // A golden helmet ties with another golden helmet, so damage and enchantments decide.
        let helmet = stack("minecraft:golden_helmet");
        let chest = stack("minecraft:golden_chestplate");
        assert_eq!(armor_points(&helmet, "head"), 2.0);
        assert_eq!(armor_points(&chest, "chest"), 5.0);
        assert!(better_armor(&chest, &helmet, "chest"));
        assert!(!better_armor(&helmet, &chest, "chest"));
        let mut worn = helmet.clone();
        worn.components
            .insert(component_ids::DAMAGE, Component::Damage(40));
        assert!(better_armor(&helmet, &worn, "head"));
        assert!(!better_armor(&worn, &helmet, "head"));
        let mut enchanted = helmet.clone();
        let mut ench = ferrite_item::Enchantments::default();
        ench.levels.insert("minecraft:protection".into(), 1);
        enchanted
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(ench));
        assert!(better_armor(&enchanted, &helmet, "head"));
        assert!(!better_armor(&helmet, &enchanted, "head"));
        assert_eq!(enchantment_count(&enchanted), 1);
        let mut bound = helmet.clone();
        let mut curse = ferrite_item::Enchantments::default();
        curse.levels.insert("minecraft:binding_curse".into(), 1);
        bound
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(curse));
        assert!(!better_armor(&chest, &bound, "head"));
    }

    #[test]
    fn a_raised_shield_faces_the_attacker() {
        let player = Vec3::new(0.0, 64.0, 0.0);
        // Yaw zero looks south; an attacker in front is blocked, one behind is not.
        assert!(shield_blocks(0.0, player, Vec3::new(0.0, 64.0, 3.0)));
        assert!(!shield_blocks(0.0, player, Vec3::new(0.0, 64.0, -3.0)));
        assert!(!shield_blocks(0.0, player, Vec3::new(0.0, 64.0, -0.2)));
        // A player facing east blocks an attacker to the east.
        assert!(shield_blocks(-90.0, player, Vec3::new(3.0, 64.0, 0.0)));
    }

    #[test]
    fn arrows_scale_with_draw_and_criticals() {
        assert_eq!(arrow_damage(0.0, false), ARROW_BASE_DAMAGE);
        assert_eq!(arrow_damage(1.0, false), 6.0);
        assert!(arrow_damage(1.0, true) > arrow_damage(1.0, false));
        assert_eq!(arrow_damage(2.0, false), arrow_damage(1.0, false));
        // Power V adds +3.0 base damage -> 5.0 * 3.0 = 15.0 at full draw (non-crit).
        assert_eq!(arrow_damage_with_power(1.0, false, 5), 15.0);
    }

    #[test]
    fn melee_enchantments_armor_defense_and_protection_follow_vanilla_formulas() {
        use ferrite_item::Enchantments;

        let mut sword = stack("minecraft:diamond_sword");
        let mut ench = Enchantments::default();
        ench.levels.insert("minecraft:sharpness".into(), 5);
        ench.levels.insert("minecraft:smite".into(), 3);
        ench.levels.insert("minecraft:bane_of_arthropods".into(), 2);
        sword
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(ench));

        // Sharpness V = 0.5*5 + 0.5 = 3.0; Smite III = 7.5 vs undead; Bane II = 5.0 vs arthropod.
        assert_eq!(melee_enchantment_bonus(&sword, false, false, 0.0), 3.0);
        assert_eq!(melee_enchantment_bonus(&sword, true, false, 0.0), 10.5);
        assert_eq!(melee_enchantment_bonus(&sword, false, true, 0.0), 8.0);
        assert!((sweeping_damage(8.0, 3) - 7.0).abs() < 1e-6);

        // Full diamond armor gives 20 defense and 8 toughness.
        let mut helmet = stack("minecraft:diamond_helmet");
        let chest = stack("minecraft:diamond_chestplate");
        let legs = stack("minecraft:diamond_leggings");
        let mut boots = stack("minecraft:diamond_boots");
        let mut prot = Enchantments::default();
        prot.levels.insert("minecraft:protection".into(), 4);
        prot.levels.insert("minecraft:thorns".into(), 2);
        helmet
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(prot));
        let mut ff = Enchantments::default();
        ff.levels.insert("minecraft:feather_falling".into(), 4);
        boots
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(ff));

        let armor = armor_stats(&[&boots, &legs, &chest, &helmet]);
        assert_eq!(armor.defense, 20.0);
        assert_eq!(armor.toughness, 8.0);
        assert_eq!(armor.epf_generic, 4);
        assert_eq!(armor.epf_fall, 12);
        assert_eq!(armor.thorns_total, 2);

        // 10 damage against 20 armor / 8 toughness: f=4, reduction = max(20 - 2.5, 4) = 17.5 -> 10 * (1 - 17.5/25) = 3.0.
        assert!((damage_after_armor(10.0, armor.defense, armor.toughness) - 3.0).abs() < 1e-5);
        // EPF 16 (Prot IV + Feather Falling IV) reduces fall damage by 16/25 = 64%.
        assert!(
            (damage_after_protection(10.0, armor.epf_generic + armor.epf_fall) - 3.6).abs() < 1e-5
        );

        // Remaining 12 vanilla 26.3 enchantments + leather armor freeze immunity.
        let mut pick = stack("minecraft:diamond_pickaxe");
        let mut eff = Enchantments::default();
        eff.levels.insert("minecraft:efficiency".into(), 5);
        pick.components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(eff));
        assert_eq!(mining_speed_multiplier(&pick), 8.0 + 26.0);

        let mut xbow = stack("minecraft:crossbow");
        let mut xench = Enchantments::default();
        xench.levels.insert("minecraft:quick_charge".into(), 3);
        xench.levels.insert("minecraft:multishot".into(), 1);
        xench.levels.insert("minecraft:piercing".into(), 4);
        xbow.components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(xench));
        assert_eq!(crossbow_charge_ticks(&xbow), 10);
        assert!(multishot_enabled(&xbow));
        assert_eq!(piercing_level(&xbow), 4);

        let mut trident = stack("minecraft:trident");
        let mut tench = Enchantments::default();
        tench.levels.insert("minecraft:impaling".into(), 5);
        trident
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(tench));
        assert_eq!(impaling_bonus(&trident, true), 12.5);
        assert_eq!(impaling_bonus(&trident, false), 0.0);

        let mut mace = stack("minecraft:mace");
        let mut mench = Enchantments::default();
        mench.levels.insert("minecraft:wind_burst".into(), 3);
        mench.levels.insert("minecraft:lunge".into(), 2);
        mace.components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(mench));
        assert_eq!(wind_burst_level(&mace), 3);
        assert_eq!(lunge_level(&mace), 2);

        let mut leather_boots = stack("minecraft:leather_boots");
        let mut bench = Enchantments::default();
        bench.levels.insert("minecraft:respiration".into(), 3);
        bench.levels.insert("minecraft:depth_strider".into(), 3);
        bench.levels.insert("minecraft:frost_walker".into(), 2);
        bench.levels.insert("minecraft:swift_sneak".into(), 3);
        bench.levels.insert("minecraft:binding_curse".into(), 1);
        leather_boots
            .components
            .insert(component_ids::ENCHANTMENTS, Component::Enchantments(bench));
        assert_eq!(respiration_level(&[&leather_boots]), 3);
        assert_eq!(depth_strider_level(&[&leather_boots]), 3);
        assert_eq!(frost_walker_level(&[&leather_boots]), 2);
        assert_eq!(swift_sneak_level(&[&leather_boots]), 3);
        assert!(has_binding_curse(&leather_boots));
        assert!(is_wearing_leather_armor(&[&leather_boots]));
    }
}
