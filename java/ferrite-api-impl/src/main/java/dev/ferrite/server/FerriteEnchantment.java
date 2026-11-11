package dev.ferrite.server;
final class FerriteEnchantment extends org.bukkit.enchantments.Enchantment {
    private final org.bukkit.NamespacedKey key;
    FerriteEnchantment(org.bukkit.NamespacedKey key) { this.key = key; }
    public org.bukkit.NamespacedKey getKey() { return key; }
    public String getName() { return key.getKey().toUpperCase(java.util.Locale.ROOT); }
    public String translationKey() { return "enchantment." + key.getNamespace() + "." + key.getKey(); }
    public String getTranslationKey() { return translationKey(); }
    public int getMaxLevel() { return FerriteEnchantmentData.get(key).maxLevel(); }
    public int getStartLevel() { return FerriteEnchantmentData.get(key).startLevel(); }
    public org.bukkit.enchantments.EnchantmentTarget getItemTarget() {
        var target = FerriteEnchantmentData.get(key).itemTarget();
        if (target == null) throw new UnsupportedOperationException("Ferrite: enchantment " + key + " has no legacy item target");
        return target;
    }
    public boolean isTreasure() { return FerriteEnchantmentData.get(key).treasure(); }
    public boolean isCursed() { return FerriteEnchantmentData.get(key).cursed(); }
    /** Vanilla {@code Enchantment.areCompatible}: an enchantment never combines with itself or its exclusive set. */
    public boolean conflictsWith(org.bukkit.enchantments.Enchantment other) {
        java.util.Objects.requireNonNull(other, "other");
        if (other.getKey().equals(key)) return true;
        return FerriteEnchantmentData.get(key).exclusiveWith().values().stream().anyMatch(exclusive -> exclusive.key().equals(other.getKey()));
    }
    public boolean canEnchantItem(org.bukkit.inventory.ItemStack item) {
        java.util.Objects.requireNonNull(item, "item");
        var type = item.getType().getKey();
        return FerriteEnchantmentData.get(key).supportedItems().values().stream().anyMatch(supported -> supported.key().equals(type));
    }
    public net.kyori.adventure.text.Component displayName(int level) {
        var definition = FerriteEnchantmentData.get(key);
        var name = definition.description().color(definition.cursed()
                ? net.kyori.adventure.text.format.NamedTextColor.RED
                : net.kyori.adventure.text.format.NamedTextColor.GRAY);
        if (level != 1 || definition.maxLevel() != 1) {
            name = name.append(net.kyori.adventure.text.Component.text(" "))
                    .append(net.kyori.adventure.text.Component.translatable("enchantment.level." + level));
        }
        return name;
    }
    public boolean isTradeable() { return HostBridge.callBoolean("Enchantment", key, "isTradeable()"); }
    public boolean isDiscoverable() { return HostBridge.callBoolean("Enchantment", key, "isDiscoverable()"); }
    public int getMinModifiedCost(int level) { return FerriteEnchantmentData.get(key).minCost().calculate(level); }
    public int getMaxModifiedCost(int level) { return FerriteEnchantmentData.get(key).maxCost().calculate(level); }
    public int getAnvilCost() { return FerriteEnchantmentData.get(key).anvilCost(); }
    public io.papermc.paper.enchantments.EnchantmentRarity getRarity() { return FerriteEnchantmentData.rarity(FerriteEnchantmentData.get(key), key); }
    public float getDamageIncrease(int level, org.bukkit.entity.EntityCategory category) { return HostBridge.callFloat("Enchantment", key, "getDamageIncrease(int,EntityCategory)", level, category); }
    public float getDamageIncrease(int level, org.bukkit.entity.EntityType type) { return HostBridge.callFloat("Enchantment", key, "getDamageIncrease(int,EntityType)", level, type); }
    public java.util.Set<org.bukkit.inventory.EquipmentSlotGroup> getActiveSlotGroups() { return FerriteEnchantmentData.get(key).slots(); }
    public net.kyori.adventure.text.Component description() { return FerriteEnchantmentData.get(key).description(); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType> getSupportedItems() { return FerriteEnchantmentData.get(key).supportedItems(); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.inventory.ItemType> getPrimaryItems() { return FerriteEnchantmentData.get(key).primaryItems(); }
    public int getWeight() { return FerriteEnchantmentData.get(key).weight(); }
    public io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment> getExclusiveWith() { return FerriteEnchantmentData.get(key).exclusiveWith(); }
}
