package org.bukkit.craftbukkit.inventory;

/**
 * CraftBukkit's item-stack conversions, backed by Ferrite's {@code net.minecraft.world.item.ItemStack}.
 *
 * <p>The NMS stack carries the whole Bukkit stack (material, amount and every metadata field), so
 * {@link #asNMSCopy} / {@link #asBukkitCopy} preserve name, lore, enchantments, damage, model data,
 * flags, persistent data and the specialised metadata (books, maps, potions, banners, fireworks,
 * skulls). A mirror returned by {@link #asCraftMirror} writes every change back through to the NMS
 * stack, which is what plugins that build packets rely on.
 */
public class CraftItemStack extends org.bukkit.inventory.ItemStack {
    /** The NMS stack this mirror writes to, or null for an ordinary copy. */
    private final net.minecraft.world.item.ItemStack handle;

    public CraftItemStack(org.bukkit.inventory.ItemStack stack) {
        super(java.util.Objects.requireNonNull(stack, "stack"));
        // Never leave a non-mirror without a handle: the backing Bukkit stack may itself be a
        // CraftItemStack, and a null handle would route its setters back into this object forever.
        this.handle = net.minecraft.world.item.ItemStack.of(stack);
    }

    public CraftItemStack(org.bukkit.Material type) { this(new org.bukkit.inventory.ItemStack(type)); }
    public CraftItemStack(org.bukkit.Material type, int amount) { this(new org.bukkit.inventory.ItemStack(type, amount)); }
    private CraftItemStack(net.minecraft.world.item.ItemStack handle) {
        super((handle == null ? net.minecraft.world.item.ItemStack.EMPTY : handle).toBukkit());
        this.handle = handle == null ? net.minecraft.world.item.ItemStack.EMPTY : handle;
    }

    /** A mirror of {@code stack}: changes to this object are visible through the NMS stack. */
    public static CraftItemStack asCraftMirror(net.minecraft.world.item.ItemStack stack) {
        return new CraftItemStack(stack == null ? net.minecraft.world.item.ItemStack.EMPTY : stack);
    }

    public static net.minecraft.world.item.ItemStack asNMSCopy(org.bukkit.inventory.ItemStack stack) {
        return net.minecraft.world.item.ItemStack.of(stack);
    }

    /** The Bukkit copy of an NMS stack; null when the NMS stack is null or empty, like Paper. */
    public static org.bukkit.inventory.ItemStack asBukkitCopy(net.minecraft.world.item.ItemStack stack) {
        return stack == null || stack.isEmpty() ? null : stack.toBukkit();
    }

    public static org.bukkit.inventory.ItemStack asCraftCopy(org.bukkit.inventory.ItemStack stack) {
        return stack == null ? null : stack.clone();
    }

    /** Bukkit-to-Bukkit conversion kept for compatibility; it is a copy, never a live view. */
    public static org.bukkit.inventory.ItemStack asBukkitCopy(org.bukkit.inventory.ItemStack stack) {
        return asCraftCopy(stack);
    }

    public static org.bukkit.inventory.ItemStack asCraftMirror(org.bukkit.inventory.ItemStack stack) {
        return asCraftCopy(stack);
    }

    private org.bukkit.inventory.ItemStack view() { return handle == null ? this : handle.view(); }
    private void mutate(java.util.function.Consumer<org.bukkit.inventory.ItemStack> edit) {
        if (handle == null) { edit.accept(this); return; }
        edit.accept(handle.view());
    }

    @Override public org.bukkit.Material getType() { return view().getType(); }
    @Override public void setType(org.bukkit.Material type) { mutate(stack -> stack.setType(type)); }
    @Override public int getAmount() { return view().getAmount(); }
    @Override public void setAmount(int amount) { mutate(stack -> stack.setAmount(amount)); }
    @Override public org.bukkit.inventory.meta.ItemMeta getItemMeta() { return view().getItemMeta(); }
    @Override public boolean setItemMeta(org.bukkit.inventory.meta.ItemMeta meta) { var result = new boolean[1]; mutate(stack -> result[0] = stack.setItemMeta(meta)); return result[0]; }
    @Override public boolean hasItemMeta() { return view().hasItemMeta(); }
    @Override public boolean isEmpty() { return view().isEmpty(); }
    @Override public int getMaxStackSize() { return view().getMaxStackSize(); }
    @Override public java.util.Map<org.bukkit.enchantments.Enchantment, Integer> getEnchantments() { return view().getEnchantments(); }
    @Override public boolean containsEnchantment(org.bukkit.enchantments.Enchantment enchantment) { return view().containsEnchantment(enchantment); }
    @Override public int getEnchantmentLevel(org.bukkit.enchantments.Enchantment enchantment) { return view().getEnchantmentLevel(enchantment); }
    @Override public void addUnsafeEnchantment(org.bukkit.enchantments.Enchantment enchantment, int level) { mutate(stack -> stack.addUnsafeEnchantment(enchantment, level)); }
    @Override public int removeEnchantment(org.bukkit.enchantments.Enchantment enchantment) { var level = new int[1]; mutate(stack -> level[0] = stack.removeEnchantment(enchantment)); return level[0]; }
    @Override public java.util.List<net.kyori.adventure.text.Component> lore() { return view().lore(); }
    @Override public void lore(java.util.List<? extends net.kyori.adventure.text.Component> lore) { mutate(stack -> stack.lore(lore)); }
    @Override public java.util.List<String> getLore() { return view().getLore(); }
    @Override public void setLore(java.util.List<String> lore) { mutate(stack -> stack.setLore(lore)); }
    @Override public void addItemFlags(org.bukkit.inventory.ItemFlag... flags) { mutate(stack -> stack.addItemFlags(flags)); }
    @Override public void removeItemFlags(org.bukkit.inventory.ItemFlag... flags) { mutate(stack -> stack.removeItemFlags(flags)); }
    @Override public java.util.Set<org.bukkit.inventory.ItemFlag> getItemFlags() { return view().getItemFlags(); }
    @Override public boolean hasItemFlag(org.bukkit.inventory.ItemFlag flag) { return view().hasItemFlag(flag); }
    @Override public boolean editMeta(java.util.function.Consumer<? super org.bukkit.inventory.meta.ItemMeta> consumer) { var result = new boolean[1]; mutate(stack -> result[0] = stack.editMeta(consumer)); return result[0]; }
    @Override public boolean isSimilar(org.bukkit.inventory.ItemStack other) {
        if (handle == null) return super.isSimilar(other);
        return other instanceof CraftItemStack mirror && mirror.handle == handle || view().isSimilar(other);
    }
    @Override public boolean equals(Object other) {
        if (handle == null) return super.equals(other);
        return other instanceof CraftItemStack mirror && mirror.handle == handle || view().equals(other);
    }
    @Override public int hashCode() { return handle == null ? super.hashCode() : view().hashCode(); }
    @Override public CraftItemStack clone() {
        return new CraftItemStack(handle.copy());
    }
    @Override public String toString() { return view().toString(); }
}
