package dev.ferrite.server;

final class FerriteItemMeta implements java.lang.reflect.InvocationHandler {
    private net.kyori.adventure.text.Component display;
    private java.util.List<net.kyori.adventure.text.Component> lore;
    private Integer model;
    private FerriteDataContainer data = new FerriteDataContainer();
    private final java.util.Map<org.bukkit.enchantments.Enchantment, Integer> enchants = new java.util.HashMap<>();
    private static final net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer LEGACY = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection();
    static org.bukkit.inventory.meta.ItemMeta create() { return new FerriteItemMeta().proxy(); }
    private org.bukkit.inventory.meta.ItemMeta proxy() { return (org.bukkit.inventory.meta.ItemMeta) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.meta.ItemMeta.class}, this); }
    private org.bukkit.inventory.meta.ItemMeta copy() { var c = new FerriteItemMeta(); c.display = display; c.lore = lore == null ? null : java.util.List.copyOf(lore); c.model = model; c.data = data.copy(); c.enchants.putAll(enchants); return c.proxy(); }
    @SuppressWarnings("unchecked") public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "hasDisplayName", "hasCustomName" -> display != null;
            case "displayName", "customName" -> { if (args == null || args.length == 0) yield display; display = (net.kyori.adventure.text.Component) args[0]; yield null; }
            case "getDisplayName" -> display == null ? "" : LEGACY.serialize(display);
            case "setDisplayName" -> { display = args[0] == null ? null : LEGACY.deserialize((String) args[0]); yield null; }
            case "hasLore" -> lore != null && !lore.isEmpty();
            case "lore" -> { if (args == null || args.length == 0) yield lore == null ? null : java.util.List.copyOf(lore); lore = args[0] == null ? null : java.util.List.copyOf((java.util.List<net.kyori.adventure.text.Component>) args[0]); yield null; }
            case "getLore" -> lore == null ? null : lore.stream().map(LEGACY::serialize).toList();
            case "setLore" -> { lore = args[0] == null ? null : ((java.util.List<String>) args[0]).stream().<net.kyori.adventure.text.Component>map(LEGACY::deserialize).toList(); yield null; }
            case "hasCustomModelData" -> model != null;
            case "getCustomModelData" -> { if (model == null) throw new IllegalStateException("No custom model data"); yield model; }
            case "setCustomModelData" -> { model = (Integer) args[0]; yield null; }
            case "hasEnchants" -> !enchants.isEmpty();
            case "hasEnchant" -> enchants.containsKey(args[0]);
            case "getEnchantLevel" -> enchants.getOrDefault(args[0], 0);
            case "getEnchants" -> java.util.Map.copyOf(enchants);
            case "addEnchant" -> { Integer old = enchants.put((org.bukkit.enchantments.Enchantment) args[0], (Integer) args[1]); yield !java.util.Objects.equals(old, args[1]); }
            case "removeEnchant" -> enchants.remove(args[0]) != null;
            case "removeEnchantments" -> { enchants.clear(); yield null; }
            case "getPersistentDataContainer" -> data;
            case "clone" -> copy();
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> "FerriteItemMeta";
            default -> HostBridge.dispatch("ItemMeta", "", method, args);
        };
    }
}
