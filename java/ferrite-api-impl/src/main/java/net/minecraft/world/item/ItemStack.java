package net.minecraft.world.item;

/** Item stack as NMS names it; holds a private Bukkit stack so the two views never alias. */
public final class ItemStack {
    public static final ItemStack EMPTY = new ItemStack(new org.bukkit.inventory.ItemStack(org.bukkit.Material.AIR));
    private final org.bukkit.inventory.ItemStack stack;
    private ItemStack(org.bukkit.inventory.ItemStack stack) { this.stack = stack; }
    public static ItemStack of(org.bukkit.inventory.ItemStack stack) { return stack == null ? EMPTY : new ItemStack(stack.clone()); }
    public boolean isEmpty() { return stack.getType().isAir() || stack.getAmount() <= 0; }
    public int getCount() { return isEmpty() ? 0 : stack.getAmount(); }
    public void setCount(int count) { stack.setAmount(count); }
    public void shrink(int count) { stack.setAmount(stack.getAmount() - count); }
    public void grow(int count) { stack.setAmount(stack.getAmount() + count); }
    public int getMaxStackSize() { return stack.getMaxStackSize(); }
    public ItemStack copy() { return new ItemStack(stack.clone()); }
    public org.bukkit.inventory.ItemStack toBukkit() { return stack.clone(); }
    @Override public String toString() { return getCount() + " " + stack.getType().getKey(); }
}
