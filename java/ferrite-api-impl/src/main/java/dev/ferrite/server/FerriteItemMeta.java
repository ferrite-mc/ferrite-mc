package dev.ferrite.server;

final class FerriteItemMeta implements java.lang.reflect.InvocationHandler {
    private net.kyori.adventure.text.Component display;
    private java.util.List<net.kyori.adventure.text.Component> lore;
    private Integer model;
    private FerriteDataContainer data = new FerriteDataContainer();
    private final java.util.Map<org.bukkit.enchantments.Enchantment, Integer> enchants = new java.util.HashMap<>();
    private final java.util.Set<org.bukkit.inventory.ItemFlag> flags = java.util.EnumSet.noneOf(org.bukkit.inventory.ItemFlag.class);
    private boolean unbreakable;
    private Integer damage;
    private int repairCost;
    private static final net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer LEGACY = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection();
    static org.bukkit.inventory.meta.ItemMeta create() { return new FerriteItemMeta().proxy(); }
    private org.bukkit.inventory.meta.ItemMeta proxy() { return (org.bukkit.inventory.meta.ItemMeta) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.meta.ItemMeta.class, org.bukkit.inventory.meta.Damageable.class}, this); }
    private org.bukkit.inventory.meta.ItemMeta copy() { var c = new FerriteItemMeta(); c.display = display; c.lore = lore == null ? null : java.util.List.copyOf(lore); c.model = model; c.data = data.copy(); c.enchants.putAll(enchants); c.flags.addAll(flags); c.unbreakable = unbreakable; c.damage = damage; c.repairCost = repairCost; return c.proxy(); }
    private boolean sameAs(FerriteItemMeta o) {
        return java.util.Objects.equals(display, o.display) && java.util.Objects.equals(lore, o.lore) && java.util.Objects.equals(model, o.model)
            && enchants.equals(o.enchants) && flags.equals(o.flags) && unbreakable == o.unbreakable
            && java.util.Objects.equals(damage == null ? 0 : damage, o.damage == null ? 0 : o.damage) && repairCost == o.repairCost && data.sameAs(o.data);
    }
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
            case "addItemFlags" -> { for (var flag : (org.bukkit.inventory.ItemFlag[]) args[0]) flags.add(flag); yield null; }
            case "removeItemFlags" -> { for (var flag : (org.bukkit.inventory.ItemFlag[]) args[0]) flags.remove(flag); yield null; }
            case "getItemFlags" -> java.util.Set.copyOf(flags);
            case "hasItemFlag" -> flags.contains(args[0]);
            case "isUnbreakable" -> unbreakable;
            case "setUnbreakable" -> { unbreakable = (boolean) args[0]; yield null; }
            case "hasDamage" -> damage != null && damage > 0;
            case "getDamage" -> damage == null ? 0 : damage;
            case "setDamage" -> { damage = (int) args[0]; yield null; }
            case "hasRepairCost" -> repairCost > 0;
            case "getRepairCost" -> repairCost;
            case "setRepairCost" -> { repairCost = (int) args[0]; yield null; }
            case "hashCode" -> java.util.Objects.hash(display, lore, model, enchants, flags, unbreakable, damage, repairCost);
            case "equals" -> self == args[0] || (args[0] != null && java.lang.reflect.Proxy.isProxyClass(args[0].getClass()) && java.lang.reflect.Proxy.getInvocationHandler(args[0]) instanceof FerriteItemMeta o && sameAs(o));
            case "toString" -> "FerriteItemMeta{" + (display == null ? "" : "name=" + display + ", ") + "enchants=" + enchants + ", flags=" + flags + "}";
            default -> HostBridge.dispatch("ItemMeta", "", method, args);
        };
    }
}
