package dev.ferrite.server;
final class FerritePotionEffectType extends org.bukkit.potion.PotionEffectType {
    private final org.bukkit.NamespacedKey key;
    FerritePotionEffectType(org.bukkit.NamespacedKey key) { this.key = key; }
    public org.bukkit.NamespacedKey getKey() { return key; }
    public String getName() { return key.getKey().toUpperCase(java.util.Locale.ROOT); }
    public String translationKey() { return "effect." + key.getNamespace() + "." + key.getKey(); }
    public String getTranslationKey() { return translationKey(); }
    private UnsupportedOperationException unsupported() { return new UnsupportedOperationException("Ferrite: effect properties not implemented: " + key); }
    public org.bukkit.potion.PotionEffect createEffect(int duration, int amplifier) { return new org.bukkit.potion.PotionEffect(this, duration, amplifier); }
    public boolean isInstant() { return HostBridge.callBoolean("PotionEffectType", key, "isInstant()"); }
    public org.bukkit.potion.PotionEffectTypeCategory getCategory() { return (org.bukkit.potion.PotionEffectTypeCategory) HostBridge.call("PotionEffectType", key, "getCategory()", org.bukkit.potion.PotionEffectTypeCategory.class); }
    public org.bukkit.Color getColor() { return (org.bukkit.Color) HostBridge.call("PotionEffectType", key, "getColor()", org.bukkit.Color.class); }
    public double getDurationModifier() { return HostBridge.callDouble("PotionEffectType", key, "getDurationModifier()"); }
    public int getId() { return HostBridge.callInt("PotionEffectType", key, "getId()"); }
    public java.util.Map<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier> getEffectAttributes() { return (java.util.Map<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier>) HostBridge.call("PotionEffectType", key, "getEffectAttributes()", java.util.Map.class); }
    public double getAttributeModifierAmount(org.bukkit.attribute.Attribute attribute, int amplifier) { return HostBridge.callDouble("PotionEffectType", key, "getAttributeModifierAmount(Attribute,int)", attribute, amplifier); }
    public org.bukkit.potion.PotionEffectType.Category getEffectCategory() { return (org.bukkit.potion.PotionEffectType.Category) HostBridge.call("PotionEffectType", key, "getEffectCategory()", org.bukkit.potion.PotionEffectType.Category.class); }
    public boolean isEnabledByFeature(org.bukkit.World world) { return HostBridge.callBoolean("PotionEffectType", key, "isEnabledByFeature(World)", world); }
}
