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
    private UnsupportedOperationException unsupported() { return new UnsupportedOperationException("Ferrite: effect properties not implemented: " + key); }
    public org.bukkit.potion.PotionEffect createEffect(int duration, int amplifier) { return new org.bukkit.potion.PotionEffect(this, duration, amplifier); }
    public boolean isInstant() {
        return key.getNamespace().equals("minecraft") && INSTANT_EFFECTS.contains(key.getKey());
    }
    public org.bukkit.potion.PotionEffectTypeCategory getCategory() { return FerritePotionEffectData.get(key).category(); }
    public org.bukkit.Color getColor() { return org.bukkit.Color.fromRGB(FerritePotionEffectData.get(key).color()); }
    public double getDurationModifier() { return HostBridge.callDouble("PotionEffectType", key, "getDurationModifier()"); }
    public int getId() { return HostBridge.callInt("PotionEffectType", key, "getId()"); }
    public java.util.Map<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier> getEffectAttributes() { return FerritePotionEffectData.get(key).attributes(); }
    public double getAttributeModifierAmount(org.bukkit.attribute.Attribute attribute, int amplifier) { return HostBridge.callDouble("PotionEffectType", key, "getAttributeModifierAmount(Attribute,int)", attribute, amplifier); }
    public org.bukkit.potion.PotionEffectType.Category getEffectCategory() { return FerritePotionEffectData.get(key).legacyCategory(); }
    public boolean isEnabledByFeature(org.bukkit.World world) { return HostBridge.callBoolean("PotionEffectType", key, "isEnabledByFeature(World)", world); }
}
