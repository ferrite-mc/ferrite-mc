package dev.ferrite.server;

import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import org.bukkit.NamespacedKey;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.potion.PotionType;

/**
 * Vanilla 26.3 potion effects, keyed by the {@code minecraft:potion} registry name.
 *
 * <p>Paper's {@link PotionType} asks {@code UnsafeValues#getInternalPotionData} for the effects of
 * each enum constant. The data mirrors the host's {@code ferrite_entity::potion} table; it is
 * repeated here because {@code PotionType} is also used before the Rust bridge is available (for
 * example in unit tests). Variant flags are derived exactly like CraftBukkit's
 * {@code CraftPotionType}: a potion is upgradeable when {@code strong_<name>} exists and
 * extendable when {@code long_<name>} exists.
 */
final class FerritePotionTypeData {
    private static final Map<String, Data> POTIONS = create();

    private FerritePotionTypeData() {}

    static PotionType.InternalPotionData get(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        var data = POTIONS.get(key.getKey());
        if (data == null) throw new IllegalArgumentException("Unknown potion type: " + key);
        return data;
    }

    private static Map<String, Data> create() {
        var effects = new HashMap<String, List<PotionEffect>>();
        put(effects, "water");
        put(effects, "mundane");
        put(effects, "thick");
        put(effects, "awkward");
        put(effects, "night_vision", effect(PotionEffectType.NIGHT_VISION, 3600, 0));
        put(effects, "long_night_vision", effect(PotionEffectType.NIGHT_VISION, 9600, 0));
        put(effects, "invisibility", effect(PotionEffectType.INVISIBILITY, 3600, 0));
        put(effects, "long_invisibility", effect(PotionEffectType.INVISIBILITY, 9600, 0));
        put(effects, "leaping", effect(PotionEffectType.JUMP_BOOST, 3600, 0));
        put(effects, "long_leaping", effect(PotionEffectType.JUMP_BOOST, 9600, 0));
        put(effects, "strong_leaping", effect(PotionEffectType.JUMP_BOOST, 1800, 1));
        put(effects, "fire_resistance", effect(PotionEffectType.FIRE_RESISTANCE, 3600, 0));
        put(effects, "long_fire_resistance", effect(PotionEffectType.FIRE_RESISTANCE, 9600, 0));
        put(effects, "swiftness", effect(PotionEffectType.SPEED, 3600, 0));
        put(effects, "long_swiftness", effect(PotionEffectType.SPEED, 9600, 0));
        put(effects, "strong_swiftness", effect(PotionEffectType.SPEED, 1800, 1));
        put(effects, "slowness", effect(PotionEffectType.SLOWNESS, 1800, 0));
        put(effects, "long_slowness", effect(PotionEffectType.SLOWNESS, 4800, 0));
        put(effects, "strong_slowness", effect(PotionEffectType.SLOWNESS, 400, 3));
        put(effects, "turtle_master",
                effect(PotionEffectType.SLOWNESS, 400, 3),
                effect(PotionEffectType.RESISTANCE, 400, 2));
        put(effects, "long_turtle_master",
                effect(PotionEffectType.SLOWNESS, 800, 3),
                effect(PotionEffectType.RESISTANCE, 800, 2));
        put(effects, "strong_turtle_master",
                effect(PotionEffectType.SLOWNESS, 400, 5),
                effect(PotionEffectType.RESISTANCE, 400, 3));
        put(effects, "water_breathing", effect(PotionEffectType.WATER_BREATHING, 3600, 0));
        put(effects, "long_water_breathing", effect(PotionEffectType.WATER_BREATHING, 9600, 0));
        put(effects, "healing", effect(PotionEffectType.INSTANT_HEALTH, 1, 0));
        put(effects, "strong_healing", effect(PotionEffectType.INSTANT_HEALTH, 1, 1));
        put(effects, "harming", effect(PotionEffectType.INSTANT_DAMAGE, 1, 0));
        put(effects, "strong_harming", effect(PotionEffectType.INSTANT_DAMAGE, 1, 1));
        put(effects, "poison", effect(PotionEffectType.POISON, 900, 0));
        put(effects, "long_poison", effect(PotionEffectType.POISON, 1800, 0));
        put(effects, "strong_poison", effect(PotionEffectType.POISON, 432, 1));
        put(effects, "regeneration", effect(PotionEffectType.REGENERATION, 900, 0));
        put(effects, "long_regeneration", effect(PotionEffectType.REGENERATION, 1800, 0));
        put(effects, "strong_regeneration", effect(PotionEffectType.REGENERATION, 450, 1));
        put(effects, "strength", effect(PotionEffectType.STRENGTH, 3600, 0));
        put(effects, "long_strength", effect(PotionEffectType.STRENGTH, 9600, 0));
        put(effects, "strong_strength", effect(PotionEffectType.STRENGTH, 1800, 1));
        put(effects, "weakness", effect(PotionEffectType.WEAKNESS, 1800, 0));
        put(effects, "long_weakness", effect(PotionEffectType.WEAKNESS, 4800, 0));
        put(effects, "luck", effect(PotionEffectType.LUCK, 6000, 0));
        put(effects, "slow_falling", effect(PotionEffectType.SLOW_FALLING, 1800, 0));
        put(effects, "long_slow_falling", effect(PotionEffectType.SLOW_FALLING, 4800, 0));
        put(effects, "wind_charged", effect(PotionEffectType.WIND_CHARGED, 3600, 0));
        put(effects, "weaving", effect(PotionEffectType.WEAVING, 3600, 0));
        put(effects, "oozing", effect(PotionEffectType.OOZING, 3600, 0));
        put(effects, "infested", effect(PotionEffectType.INFESTED, 3600, 0));

        var data = new HashMap<String, Data>();
        effects.forEach((name, potionEffects) -> data.put(name, new Data(
                name,
                potionEffects,
                effects.containsKey("strong_" + name),
                effects.containsKey("long_" + name))));
        return Map.copyOf(data);
    }

    private static PotionEffect effect(PotionEffectType type, int duration, int amplifier) {
        return new PotionEffect(type, duration, amplifier);
    }

    private static void put(Map<String, List<PotionEffect>> potions, String name, PotionEffect... effects) {
        potions.put(name, List.of(effects));
    }

    private record Data(String name, List<PotionEffect> effects, boolean upgradeable, boolean extendable)
            implements PotionType.InternalPotionData {
        @Override public PotionEffectType getEffectType() {
            return effects.isEmpty() ? null : effects.get(0).getType();
        }

        @Override public List<PotionEffect> getPotionEffects() { return effects; }
        @Override public boolean isInstant() { return effects.stream().anyMatch(effect -> effect.getType().isInstant()); }
        @Override public boolean isUpgradeable() { return upgradeable; }
        @Override public boolean isExtendable() { return extendable; }
        @Override public int getMaxLevel() { return upgradeable ? 2 : 1; }

        @Override public String toString() { return "PotionTypeData[" + name + "]"; }
    }
}
