package dev.ferrite.server;

/** Item stack metadata factory; enchantment and hover conversions need host data and stay unsupported. */
final class FerriteItemFactory implements org.bukkit.inventory.ItemFactory {
    private static final org.bukkit.Color LEATHER_COLOR = org.bukkit.Color.fromRGB(0xA06540);
    @Override public org.bukkit.inventory.meta.ItemMeta getItemMeta(org.bukkit.Material material) {
        java.util.Objects.requireNonNull(material, "material");
        return material.isItem() && !material.isAir() ? FerriteItemMeta.create() : null;
    }
    @Override public boolean isApplicable(org.bukkit.inventory.meta.ItemMeta meta, org.bukkit.inventory.ItemStack item) {
        java.util.Objects.requireNonNull(item, "item");
        return isApplicable(meta, item.getType());
    }
    @Override public boolean isApplicable(org.bukkit.inventory.meta.ItemMeta meta, org.bukkit.Material material) {
        java.util.Objects.requireNonNull(material, "material");
        return meta != null && material.isItem() && !material.isAir();
    }
    @Override public boolean equals(org.bukkit.inventory.meta.ItemMeta first, org.bukkit.inventory.meta.ItemMeta second) {
        java.util.Objects.requireNonNull(first, "first");
        java.util.Objects.requireNonNull(second, "second");
        return first == second || first.equals(second);
    }
    @Override public org.bukkit.inventory.meta.ItemMeta asMetaFor(org.bukkit.inventory.meta.ItemMeta meta, org.bukkit.inventory.ItemStack item) {
        java.util.Objects.requireNonNull(item, "item");
        return asMetaFor(meta, item.getType());
    }
    @Override public org.bukkit.inventory.meta.ItemMeta asMetaFor(org.bukkit.inventory.meta.ItemMeta meta, org.bukkit.Material material) {
        java.util.Objects.requireNonNull(meta, "meta");
        if (!isApplicable(meta, material)) throw new IllegalArgumentException("ItemMeta is not applicable to " + material);
        return meta.clone();
    }
    @Override public org.bukkit.Color getDefaultLeatherColor() { return LEATHER_COLOR; }
    @Override public org.bukkit.inventory.ItemStack createItemStack(String input) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "createItemStack(String)", org.bukkit.inventory.ItemStack.class, input); }
    @Override public org.bukkit.Material getSpawnEgg(org.bukkit.entity.EntityType type) {
        java.util.Objects.requireNonNull(type, "type");
        return org.bukkit.Material.matchMaterial(type.name() + "_SPAWN_EGG");
    }
    @Override public org.bukkit.inventory.ItemStack enchantItem(org.bukkit.entity.Entity entity, org.bukkit.inventory.ItemStack item, int level, boolean allowTreasure) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "enchantItem(Entity,ItemStack,int,boolean)", org.bukkit.inventory.ItemStack.class, entity, item, level, allowTreasure); }
    @Override public org.bukkit.inventory.ItemStack enchantItem(org.bukkit.World world, org.bukkit.inventory.ItemStack item, int level, boolean allowTreasure) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "enchantItem(World,ItemStack,int,boolean)", org.bukkit.inventory.ItemStack.class, world, item, level, allowTreasure); }
    @Override public org.bukkit.inventory.ItemStack enchantItem(org.bukkit.inventory.ItemStack item, int level, boolean allowTreasure) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "enchantItem(ItemStack,int,boolean)", org.bukkit.inventory.ItemStack.class, item, level, allowTreasure); }
    @Override public net.kyori.adventure.text.event.HoverEvent<net.kyori.adventure.text.event.HoverEvent.ShowItem> asHoverEvent(org.bukkit.inventory.ItemStack item, java.util.function.UnaryOperator<net.kyori.adventure.text.event.HoverEvent.ShowItem> operator) { return (net.kyori.adventure.text.event.HoverEvent<net.kyori.adventure.text.event.HoverEvent.ShowItem>) HostBridge.call("ItemFactory", "items", "asHoverEvent(ItemStack,UnaryOperator)", net.kyori.adventure.text.event.HoverEvent.class, item, operator); }
    @Override public net.kyori.adventure.text.Component displayName(org.bukkit.inventory.ItemStack item) {
        java.util.Objects.requireNonNull(item, "item");
        if (item.hasItemMeta() && item.getItemMeta().hasDisplayName()) {
            var name = item.getItemMeta().displayName();
            if (name != null) return name;
        }
        return net.kyori.adventure.text.Component.translatable(item.getType().translationKey());
    }
    @Override public String getI18NDisplayName(org.bukkit.inventory.ItemStack item) {
        return net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(displayName(item));
    }
    @Override public org.bukkit.inventory.ItemStack ensureServerConversions(org.bukkit.inventory.ItemStack item) { return java.util.Objects.requireNonNull(item, "item"); }
    @Override public net.md_5.bungee.api.chat.hover.content.Content hoverContentOf(org.bukkit.inventory.ItemStack item) { return (net.md_5.bungee.api.chat.hover.content.Content) HostBridge.call("ItemFactory", "items", "hoverContentOf(ItemStack)", net.md_5.bungee.api.chat.hover.content.Content.class, item); }
    @Override public net.md_5.bungee.api.chat.hover.content.Content hoverContentOf(org.bukkit.entity.Entity entity) { return (net.md_5.bungee.api.chat.hover.content.Content) HostBridge.call("ItemFactory", "items", "hoverContentOf(Entity)", net.md_5.bungee.api.chat.hover.content.Content.class, entity); }
    @Override public net.md_5.bungee.api.chat.hover.content.Content hoverContentOf(org.bukkit.entity.Entity entity, String customName) { return (net.md_5.bungee.api.chat.hover.content.Content) HostBridge.call("ItemFactory", "items", "hoverContentOf(Entity,String)", net.md_5.bungee.api.chat.hover.content.Content.class, entity, customName); }
    @Override public net.md_5.bungee.api.chat.hover.content.Content hoverContentOf(org.bukkit.entity.Entity entity, net.md_5.bungee.api.chat.BaseComponent customName) { return (net.md_5.bungee.api.chat.hover.content.Content) HostBridge.call("ItemFactory", "items", "hoverContentOf(Entity,BaseComponent)", net.md_5.bungee.api.chat.hover.content.Content.class, entity, customName); }
    @Override public net.md_5.bungee.api.chat.hover.content.Content hoverContentOf(org.bukkit.entity.Entity entity, net.md_5.bungee.api.chat.BaseComponent[] customName) { return (net.md_5.bungee.api.chat.hover.content.Content) HostBridge.call("ItemFactory", "items", "hoverContentOf(Entity,BaseComponent[])", net.md_5.bungee.api.chat.hover.content.Content.class, entity, customName); }
    @Override public org.bukkit.inventory.ItemStack enchantWithLevels(org.bukkit.inventory.ItemStack item, int levels, boolean allowTreasure, java.util.Random random) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "enchantWithLevels(ItemStack,int,boolean,Random)", org.bukkit.inventory.ItemStack.class, item, levels, allowTreasure, random); }
    @Override public org.bukkit.inventory.ItemStack enchantWithLevels(org.bukkit.inventory.ItemStack item, int levels, io.papermc.paper.registry.set.RegistryKeySet<org.bukkit.enchantments.Enchantment> enchantments, java.util.Random random) { return (org.bukkit.inventory.ItemStack) HostBridge.call("ItemFactory", "items", "enchantWithLevels(ItemStack,int,RegistryKeySet,Random)", org.bukkit.inventory.ItemStack.class, item, levels, enchantments, random); }
}
