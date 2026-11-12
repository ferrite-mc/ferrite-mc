package dev.ferrite.server;
final class FerritePotionEffectType extends org.bukkit.potion.PotionEffectType {
    /** Vanilla effects that apply once; every other mob effect is a timed effect. */
    private static final java.util.Set<String> INSTANT_EFFECTS =
            java.util.Set.of("instant_health", "instant_damage");

    private final org.bukkit.NamespacedKey key;
    FerritePotionEffectType(org.bukkit.NamespacedKey key) { this.key = key; }
    public org.bukkit.NamespacedKey getKey() { return key; }
    public String getName() { return key.getKey().toUpperCase(java.util.Locale.ROOT); }
    public String translationKey() { return "effect." + key.getNamespace() + "." + key.getKey(); }
    public String getTranslationKey() { return translationKey(); }
    public org.bukkit.potion.PotionEffect createEffect(int duration, int amplifier) { return new org.bukkit.potion.PotionEffect(this, duration, amplifier); }
    public boolean isInstant() {
        return key.getNamespace().equals("minecraft") && INSTANT_EFFECTS.contains(key.getKey());
    }
    public org.bukkit.potion.PotionEffectTypeCategory getCategory() { return FerritePotionEffectData.get(key).category(); }
    public org.bukkit.Color getColor() { return org.bukkit.Color.fromRGB(FerritePotionEffectData.get(key).color()); }
    /** Vanilla 26.3 {@code MobEffect} has no duration-modifier field, so every effect scales durations by 1. */
    public double getDurationModifier() { FerritePotionEffectData.get(key); return 1.0; }
    /** The vanilla {@code minecraft:mob_effect} protocol id from the generated registry report. */
    public int getId() {
        FerritePotionEffectData.get(key);
        int id = FerriteRegistryAccess.protocolId("mob_effect", key);
        if (id < 0) throw new UnsupportedOperationException("Ferrite: no generated protocol id for " + key);
        return id;
    }
    public java.util.Map<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier> getEffectAttributes() { return FerritePotionEffectData.get(key).attributes(); }
    /** Vanilla scales a registered modifier by {@code amplifier + 1} ({@code AttributeTemplate.create}). */
    public double getAttributeModifierAmount(org.bukkit.attribute.Attribute attribute, int amplifier) {
        java.util.Objects.requireNonNull(attribute, "attribute");
        if (amplifier < 0) throw new IllegalArgumentException("amplifier must be >= 0, was " + amplifier);
        for (var modifier : FerritePotionEffectData.get(key).modifiers())
            if (org.bukkit.NamespacedKey.minecraft(modifier.attribute()).equals(attribute.getKey()))
                return modifier.amount() * (amplifier + 1);
        throw new IllegalArgumentException("Effect " + key + " does not have attribute " + attribute.getKey());
    }
    public org.bukkit.potion.PotionEffectType.Category getEffectCategory() { return FerritePotionEffectData.get(key).legacyCategory(); }
    /** No registered 26.3 mob effect declares required features, so every effect is enabled in every world. */
    public boolean isEnabledByFeature(org.bukkit.World world) {
        java.util.Objects.requireNonNull(world, "world");
        FerritePotionEffectData.get(key);
        return true;
    }
}
