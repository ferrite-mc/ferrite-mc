package dev.ferrite.server;

/** Lightweight item stack until the server supplies full item data. */
final class FerriteItemStack extends org.bukkit.inventory.ItemStack {
    private org.bukkit.Material type;
    private int amount;
    private org.bukkit.inventory.meta.ItemMeta meta;
    FerriteItemStack(org.bukkit.Material type, int amount) { this.type = type; this.amount = amount; }
    @Override public org.bukkit.Material getType() { return type; }
    @Override public void setType(org.bukkit.Material type) { this.type = java.util.Objects.requireNonNull(type); }
    @Override public int getAmount() { return amount; }
    @Override public void setAmount(int amount) { this.amount = amount; }
    @Override public org.bukkit.inventory.meta.ItemMeta getItemMeta() { return meta == null ? FerriteItemMeta.create(type) : meta.clone(); }
    @Override public boolean setItemMeta(org.bukkit.inventory.meta.ItemMeta meta) { this.meta = meta == null ? null : meta.clone(); return true; }
    @Override public boolean hasItemMeta() { return meta != null; }
    private static boolean air(org.bukkit.Material material) { return material == org.bukkit.Material.AIR || material == org.bukkit.Material.CAVE_AIR || material == org.bukkit.Material.VOID_AIR; }
    private org.bukkit.inventory.meta.ItemMeta metaOrNull() { return meta; }
    private org.bukkit.inventory.meta.ItemMeta editable() { return meta != null ? meta : FerriteItemMeta.create(type); }
    private FerriteItemStack withAmount(int newAmount) { var result = clone(); result.amount = newAmount; return result; }
    @Override public int getMaxStackSize() { int size = type.getMaxStackSize(); return size > 0 ? size : 64; }
    @Override public boolean isEmpty() { return air(type) || amount <= 0; }
    @Override public boolean isSimilar(org.bukkit.inventory.ItemStack other) {
        if (other == null) return false;
        if (other == this) return true;
        if (type != other.getType()) return false;
        var mine = metaOrNull();
        var theirs = other.hasItemMeta() ? other.getItemMeta() : null;
        return java.util.Objects.equals(mine, theirs);
    }
    @Override public boolean equals(Object other) {
        if (this == other) return true;
        return other instanceof org.bukkit.inventory.ItemStack stack && amount == stack.getAmount() && isSimilar(stack);
    }
    @Override public int hashCode() { int hash = 1; hash = hash * 31 + type.hashCode(); hash = hash * 31 + amount; hash = hash * 31 + (meta == null ? 0 : meta.hashCode()); return hash; }
    @Override public String toString() { return "ItemStack{" + type + " x " + amount + (meta == null ? "" : ", " + meta) + "}"; }
    @Override public org.bukkit.inventory.ItemStack asOne() { return withAmount(1); }
    @Override public org.bukkit.inventory.ItemStack asQuantity(int quantity) { return withAmount(quantity); }
    @Override public org.bukkit.inventory.ItemStack add() { return add(1); }
    @Override public org.bukkit.inventory.ItemStack add(int by) { amount += by; return this; }
    @Override public org.bukkit.inventory.ItemStack subtract() { return subtract(1); }
    @Override public org.bukkit.inventory.ItemStack subtract(int by) { amount -= by; return this; }
    @Override public boolean editMeta(java.util.function.Consumer<? super org.bukkit.inventory.meta.ItemMeta> consumer) {
        if (air(type)) return false;
        var edited = editable();
        consumer.accept(edited);
        meta = edited;
        return true;
    }
    @Override public <M extends org.bukkit.inventory.meta.ItemMeta> boolean editMeta(Class<M> metaClass, java.util.function.Consumer<? super M> consumer) {
        if (air(type) || !metaClass.isInstance(editable())) return false;
        var edited = editable();
        consumer.accept(metaClass.cast(edited));
        meta = edited;
        return true;
    }
    @Override public java.util.Map<org.bukkit.enchantments.Enchantment, Integer> getEnchantments() { return meta == null ? new java.util.HashMap<>() : new java.util.HashMap<>(meta.getEnchants()); }
    @Override public boolean containsEnchantment(org.bukkit.enchantments.Enchantment enchantment) { return meta != null && meta.hasEnchant(enchantment); }
    @Override public int getEnchantmentLevel(org.bukkit.enchantments.Enchantment enchantment) { return meta == null ? 0 : meta.getEnchantLevel(enchantment); }
    @Override public void addUnsafeEnchantment(org.bukkit.enchantments.Enchantment enchantment, int level) { var edited = editable(); edited.addEnchant(enchantment, level, true); meta = edited; }
    @Override public void addUnsafeEnchantments(java.util.Map<org.bukkit.enchantments.Enchantment, Integer> enchantments) { enchantments.forEach(this::addUnsafeEnchantment); }
    @Override public void addEnchantment(org.bukkit.enchantments.Enchantment enchantment, int level) { addUnsafeEnchantment(enchantment, level); }
    @Override public void addEnchantments(java.util.Map<org.bukkit.enchantments.Enchantment, Integer> enchantments) { enchantments.forEach(this::addEnchantment); }
    @Override public int removeEnchantment(org.bukkit.enchantments.Enchantment enchantment) { int level = getEnchantmentLevel(enchantment); if (meta != null) meta.removeEnchant(enchantment); return level; }
    @Override public void removeEnchantments() { if (meta != null) for (var enchantment : new java.util.ArrayList<>(meta.getEnchants().keySet())) meta.removeEnchant(enchantment); }
    @Override public java.util.List<net.kyori.adventure.text.Component> lore() { return meta == null ? null : meta.lore(); }
    @Override public void lore(java.util.List<? extends net.kyori.adventure.text.Component> lore) { var edited = editable(); edited.lore(lore); meta = edited; }
    @Override public java.util.List<String> getLore() { return meta == null ? null : meta.getLore(); }
    @Override public void setLore(java.util.List<String> lore) { var edited = editable(); edited.setLore(lore); meta = edited; }
    @Override public void addItemFlags(org.bukkit.inventory.ItemFlag... flags) { var edited = editable(); edited.addItemFlags(flags); meta = edited; }
    @Override public void removeItemFlags(org.bukkit.inventory.ItemFlag... flags) { if (meta != null) meta.removeItemFlags(flags); }
    @Override public java.util.Set<org.bukkit.inventory.ItemFlag> getItemFlags() { return meta == null ? new java.util.HashSet<>() : meta.getItemFlags(); }
    @Override public boolean hasItemFlag(org.bukkit.inventory.ItemFlag flag) { return meta != null && meta.hasItemFlag(flag); }
    @Override public net.kyori.adventure.text.Component displayName() { return Bukkit_factory().displayName(this); }
    @Override public net.kyori.adventure.text.Component effectiveName() { return displayName(); }
    private static FerriteItemFactory Bukkit_factory() { return new FerriteItemFactory(); }
    @Override public String translationKey() { return type.translationKey(); }
    @Override public String getTranslationKey() { return type.translationKey(); }
    @Override public java.util.Map<String, Object> serialize() {
        var result = new java.util.LinkedHashMap<String, Object>();
        result.put("v", 1);
        result.put("type", type.name());
        if (amount != 1) result.put("amount", amount);
        if (meta != null) result.put("meta", meta);
        return result;
    }
    @Override public org.bukkit.inventory.ItemStack ensureServerConversions() { return this; }
    @Override public FerriteItemStack clone() { var result = new FerriteItemStack(type, amount); result.meta = meta == null ? null : meta.clone(); return result; }
}
