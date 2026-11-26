package dev.ferrite.api.server;

import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import org.bukkit.Color;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.potion.PotionEffectTypeCategory;

/** Concrete {@link PotionEffectType} backed by Ferrite's vanilla 26.3 mob_effect registry. */
@SuppressWarnings("deprecation")
final class FerritePotionEffectType extends PotionEffectType {

    private final NamespacedKey key;
    private final int id;
    private final boolean instant;

    FerritePotionEffectType(NamespacedKey key, int id) {
        this.key = Objects.requireNonNull(key, "key");
        this.id = id;
        String path = key.getKey();
        this.instant = "instant_health".equals(path) || "instant_damage".equals(path);
    }

    @Override
    public NamespacedKey getKey() {
        return key;
    }

    @Override
    public String translationKey() {
        return "effect." + key.getNamespace() + "." + key.getKey();
    }

    @Override
    public String getTranslationKey() {
        return translationKey();
    }

    @Override
    public PotionEffect createEffect(int duration, int amplifier) {
        return new PotionEffect(this, duration, amplifier);
    }

    @Override
    public boolean isInstant() {
        return instant;
    }

    @Override
    public PotionEffectTypeCategory getCategory() {
        return PotionEffectTypeCategory.NEUTRAL;
    }

    @Override
    public Color getColor() {
        return Color.WHITE;
    }

    @Override
    public double getDurationModifier() {
        return 1.0;
    }

    @Override
    public int getId() {
        return id;
    }

    @Override
    public String getName() {
        return key.getKey().toUpperCase(Locale.ROOT);
    }

    @Override
    public Map<Attribute, AttributeModifier> getEffectAttributes() {
        return Map.of();
    }

    @Override
    public double getAttributeModifierAmount(Attribute attribute, int effectAmplifier) {
        return 0.0;
    }

    @Override
    public Category getEffectCategory() {
        return Category.NEUTRAL;
    }
}
