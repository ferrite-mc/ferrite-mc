package dev.ferrite.server;
final class FerriteEnchantment extends org.bukkit.enchantments.Enchantment {
    private final org.bukkit.NamespacedKey key;
    FerriteEnchantment(org.bukkit.NamespacedKey key) { this.key = key; }
    public org.bukkit.NamespacedKey getKey() { return key; }
    public String getName() { return key.getKey().toUpperCase(java.util.Locale.ROOT); }
    public String translationKey() { return "enchantment." + key.getNamespace() + "." + key.getKey(); }
    public String getTranslationKey() { return translationKey(); }
    private UnsupportedOperationException unsupported() { return new UnsupportedOperationException("Ferrite: enchantment properties not implemented: " + key); }
    public int getMaxLevel() { return HostBridge.callInt("Enchantment", key, "getMaxLevel()"); }
    public int getStartLevel() { return HostBridge.callInt("Enchantment", key, "getStartLevel()"); }
    public org.bukkit.enchantments.EnchantmentTarget getItemTarget() { return (org.bukkit.enchantments.EnchantmentTarget) HostBridge.call("Enchantment", key, "getItemTarget()", org.bukkit.enchantments.EnchantmentTarget.class); }
    public boolean isTreasure() { return HostBridge.callBoolean("Enchantment", key, "isTreasure()"); }
    public boolean isCursed() { return HostBridge.callBoolean("Enchantment", key, "isCursed()"); }
    public boolean conflictsWith(org.bukkit.enchantments.Enchantment other) { return HostBridge.callBoolean("Enchantment", key, "conflictsWith(Enchantment)", other); }
    public boolean canEnchantItem(org.bukkit.inventory.ItemStack item) { return HostBridge.callBoolean("Enchantment", key, "canEnchantItem(ItemStack)", item); }
    public net.kyori.adventure.text.Component displayName(int level) { return (net.kyori.adventure.text.Component) HostBridge.call("Enchantment", key, "displayName(int)", net.kyori.adventure.text.Component.class, level); }
    public boolean isTradeable() { return HostBridge.callBoolean("Enchantment", key, "isTradeable()"); }
    public boolean isDiscoverable() { return HostBridge.callBoolean("Enchantment", key, "isDiscoverable()"); }
    public int getMinModifiedCost(int level) { return HostBridge.callInt("Enchantment", key, "getMinModifiedCost(int)", level); }
    public int getMaxModifiedCost(int level) { return HostBridge.callInt("Enchantment", key, "getMaxModifiedCost(int)", level); }
    public int getAnvilCost() { return HostBridge.callInt("Enchantment", key, "getAnvilCost()"); }
    public io.papermc.paper.enchantments.EnchantmentRarity getRarity() { return (io.papermc.paper.enchantments.EnchantmentRarity) HostBridge.call("Enchantment", key, "getRarity()", io.papermc.paper.enchantments.EnchantmentRarity.class); }
    public float getDamageIncrease(int level, org.bukkit.entity.EntityCategory category) { return HostBridge.callFloat("Enchantment", key, "getDamageIncrease(int,EntityCategory)", level, category); }
    public float getDamageIncrease(int level, org.bukkit.entity.EntityType type) { return HostBridge.callFloat("Enchantment", key, "getDamageIncrease(int,EntityType)", level, type); }
    public java.util.Set<org.bukkit.inventory.EquipmentSlotGroup> getActiveSlotGroups() { return (java.util.Set<org.bukkit.inventory.EquipmentSlotGroup>) HostBridge.call("Enchantment", key, "getActiveSlotGroups()", java.util.Set.class); }
    public net.kyori.adventure.text.Component description() { return (net.kyori.adventure.text.Component) HostBridge.call("Enchantment", key, "description()", net.kyori.adventure.text.Component.class); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType> getSupportedItems() { return (io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType>) HostBridge.call("Enchantment", key, "getSupportedItems()", io.papermc.paper.registry.set.RegistryKeySet.class); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType> getPrimaryItems() { return (io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType>) HostBridge.call("Enchantment", key, "getPrimaryItems()", io.papermc.paper.registry.set.RegistryKeySet.class); }
    public int getWeight() { return HostBridge.callInt("Enchantment", key, "getWeight()"); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment> getExclusiveWith() { return (io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment>) HostBridge.call("Enchantment", key, "getExclusiveWith()", io.papermc.paper.registry.set.RegistryKeySet.class); }
}
