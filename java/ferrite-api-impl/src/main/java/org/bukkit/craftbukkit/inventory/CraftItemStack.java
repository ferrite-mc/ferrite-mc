package org.bukkit.craftbukkit.inventory;

/**
 * Compatibility item-stack helper.
 *
 * <p>Real CraftBukkit exposes {@code asNMSCopy}/{@code asBukkitCopy}, but those signatures use
 * {@code net.minecraft.world.item.ItemStack}, which Ferrite cannot compile against. The
 * API-typed constructors and copies below let classes load and operate on Bukkit stacks; NMS
 * conversion is deliberately absent and listed in docs/BACKLOG.md.
 */
public class CraftItemStack extends org.bukkit.inventory.ItemStack {
    public CraftItemStack(org.bukkit.inventory.ItemStack stack) {
        super(java.util.Objects.requireNonNull(stack, "stack"));
    }

    public CraftItemStack(org.bukkit.Material type) { super(type); }
    public CraftItemStack(org.bukkit.Material type, int amount) { super(type, amount); }

    /** Bukkit-visible copy of a stack, null-safe. */
    public static org.bukkit.inventory.ItemStack asCraftCopy(org.bukkit.inventory.ItemStack stack) {
        return stack == null ? null : stack.clone();
    }

    public static net.minecraft.world.item.ItemStack asNMSCopy(org.bukkit.inventory.ItemStack stack) { return net.minecraft.world.item.ItemStack.of(stack); }
    public static org.bukkit.inventory.ItemStack asBukkitCopy(net.minecraft.world.item.ItemStack stack) { return stack.toBukkit(); }
    public static CraftItemStack asCraftMirror(net.minecraft.world.item.ItemStack stack) { return new CraftItemStack(stack.toBukkit()); }

    public static org.bukkit.inventory.ItemStack asBukkitCopy(org.bukkit.inventory.ItemStack stack) {
        return asCraftCopy(stack);
    }

    public static org.bukkit.inventory.ItemStack asCraftMirror(org.bukkit.inventory.ItemStack stack) { return asCraftCopy(stack); }
}
