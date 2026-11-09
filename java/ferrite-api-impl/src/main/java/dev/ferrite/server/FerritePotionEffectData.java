package dev.ferrite.server;

import static org.bukkit.attribute.AttributeModifier.Operation.ADD_NUMBER;
import static org.bukkit.attribute.AttributeModifier.Operation.MULTIPLY_SCALAR_1;
import static org.bukkit.potion.PotionEffectTypeCategory.BENEFICIAL;
import static org.bukkit.potion.PotionEffectTypeCategory.HARMFUL;
import static org.bukkit.potion.PotionEffectTypeCategory.NEUTRAL;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.potion.PotionEffectTypeCategory;

/**
 * Vanilla 26.3 mob-effect properties, keyed by the {@code minecraft:mob_effect} registry name.
 *
 * <p>Mob effects are a built-in registry: the data generator only reports their ids, while the
 * colour, category and attribute modifiers live in the vanilla {@code MobEffects} code. The table
 * mirrors those registrations (the host's {@code ferrite_entity::effect} carries the same
 * semantics) and is only handed out for keys present in the generated mob-effect registry.
 */
final class FerritePotionEffectData {
    private FerritePotionEffectData() {}

    /** One attribute modifier as registered by vanilla. */
    record Modifier(String attribute, String id, double amount, AttributeModifier.Operation operation) {}

    /** A vanilla effect: its RGB colour, category and base (amplifier 0) attribute modifiers. */
    record Definition(int color, PotionEffectTypeCategory category, List<Modifier> modifiers) {
        Map<Attribute, AttributeModifier> attributes() {
            if (modifiers.isEmpty()) return Map.of();
            var attributes = new LinkedHashMap<Attribute, AttributeModifier>();
            for (var modifier : modifiers) {
                var key = NamespacedKey.minecraft(modifier.attribute());
                var attribute = org.bukkit.Registry.ATTRIBUTE.get(key);
                if (attribute == null)
                    throw new UnsupportedOperationException("Ferrite: no vanilla attribute data for " + key);
                attributes.put(attribute, new AttributeModifier(
                        NamespacedKey.minecraft(modifier.id()), modifier.amount(), modifier.operation()));
            }
            return Map.copyOf(attributes);
        }

        org.bukkit.potion.PotionEffectType.Category legacyCategory() {
            return switch (category) {
                case BENEFICIAL -> org.bukkit.potion.PotionEffectType.Category.BENEFICIAL;
                case HARMFUL -> org.bukkit.potion.PotionEffectType.Category.HARMFUL;
                case NEUTRAL -> org.bukkit.potion.PotionEffectType.Category.NEUTRAL;
            };
        }
    }

    private static final Map<NamespacedKey, Definition> EFFECTS = Map.ofEntries(
            effect("speed", 0x33EBFF, BENEFICIAL, modifier("movement_speed", "effect.speed", 0.2, MULTIPLY_SCALAR_1)),
            effect("slowness", 0x8BAFE0, HARMFUL, modifier("movement_speed", "effect.slowness", -0.15, MULTIPLY_SCALAR_1)),
            effect("haste", 0xD9C043, BENEFICIAL, modifier("attack_speed", "effect.haste", 0.1, MULTIPLY_SCALAR_1)),
            effect("mining_fatigue", 0x4A4217, HARMFUL, modifier("attack_speed", "effect.mining_fatigue", -0.1, MULTIPLY_SCALAR_1)),
            effect("strength", 0xFFC700, BENEFICIAL, modifier("attack_damage", "effect.strength", 3.0, ADD_NUMBER)),
            effect("instant_health", 0xF82423, BENEFICIAL),
            effect("instant_damage", 0xA9656A, HARMFUL),
            effect("jump_boost", 0xFDFF84, BENEFICIAL, modifier("safe_fall_distance", "effect.jump_boost", 1.0, ADD_NUMBER)),
            effect("nausea", 0x551D4A, HARMFUL),
            effect("regeneration", 0xCD5CAB, BENEFICIAL),
            effect("resistance", 0x9146F0, BENEFICIAL),
            effect("fire_resistance", 0xFF9900, BENEFICIAL),
            effect("water_breathing", 0x98DAC0, BENEFICIAL),
            effect("invisibility", 0xF6F6F6, BENEFICIAL,
                    modifier("waypoint_transmit_range", "effect.waypoint_transmit_range_hide", -1.0, MULTIPLY_SCALAR_1)),
            effect("blindness", 0x1F1F23, HARMFUL),
            effect("night_vision", 0xC2FF66, BENEFICIAL),
            effect("hunger", 0x587653, HARMFUL),
            effect("weakness", 0x484D48, HARMFUL, modifier("attack_damage", "effect.weakness", -4.0, ADD_NUMBER)),
            effect("poison", 0x87A363, HARMFUL),
            effect("wither", 0x736156, HARMFUL),
            effect("health_boost", 0xF87D23, BENEFICIAL, modifier("max_health", "effect.health_boost", 4.0, ADD_NUMBER)),
            effect("absorption", 0x2552A5, BENEFICIAL, modifier("max_absorption", "effect.absorption", 4.0, ADD_NUMBER)),
            effect("saturation", 0xF82423, BENEFICIAL),
            effect("glowing", 0x94A061, NEUTRAL),
            effect("levitation", 0xCEFFFF, HARMFUL),
            effect("luck", 0x59C106, BENEFICIAL, modifier("luck", "effect.luck", 1.0, ADD_NUMBER)),
            effect("unluck", 0xC0A44D, HARMFUL, modifier("luck", "effect.unluck", -1.0, ADD_NUMBER)),
            effect("slow_falling", 0xF3CFB9, BENEFICIAL),
            effect("conduit_power", 0x1DC2D1, BENEFICIAL),
            effect("dolphins_grace", 0x88A3BE, BENEFICIAL),
            effect("bad_omen", 0x0B6138, NEUTRAL),
            effect("hero_of_the_village", 0x44FF44, BENEFICIAL),
            effect("darkness", 0x292721, HARMFUL),
            effect("trial_omen", 0x16A6A6, NEUTRAL),
            effect("raid_omen", 0xDE4058, NEUTRAL),
            effect("wind_charged", 0xBDC9FF, HARMFUL),
            effect("weaving", 0x78695A, HARMFUL),
            effect("oozing", 0x99FFA3, HARMFUL),
            effect("infested", 0x8C9B8C, HARMFUL),
            effect("breath_of_the_nautilus", 0x00FFEE, BENEFICIAL));

    static Definition get(NamespacedKey key) {
        java.util.Objects.requireNonNull(key, "key");
        if (!FerriteRegistryAccess.contains("mob_effect", key))
            throw new UnsupportedOperationException("Ferrite: no vanilla mob effect data for " + key);
        var definition = EFFECTS.get(key);
        if (definition == null)
            throw new UnsupportedOperationException("Ferrite: no vanilla mob effect data for " + key);
        return definition;
    }

    private static Map.Entry<NamespacedKey, Definition> effect(String name, int color, PotionEffectTypeCategory category, Modifier... modifiers) {
        return Map.entry(NamespacedKey.minecraft(name), new Definition(color, category, List.of(modifiers)));
    }

    private static Modifier modifier(String attribute, String id, double amount, AttributeModifier.Operation operation) {
        return new Modifier(attribute, id, amount, operation);
    }
}
