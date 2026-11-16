// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.inventory;

import dev.ferrite.nms.NmsUnsupported;
import java.util.Objects;
import net.minecraft.world.item.ItemStack;

/**
 * Clean-room item conversions for plugins that need the NMS type.
 *
 * <p>Both conversions copy. The NMS value keeps a defensive Paper copy, so display name, lore,
 * persistent data and specialised metadata survive without being reinterpreted or dropped.
 */
public final class CraftItemStack {
    private CraftItemStack() {}

    public static ItemStack asNMSCopy(org.bukkit.inventory.ItemStack stack) {
        return ItemStack.of(stack);
    }

    /** The Paper copy, or null for a null/empty NMS stack, matching CraftBukkit's contract. */
    public static org.bukkit.inventory.ItemStack asBukkitCopy(ItemStack stack) {
        return stack == null || stack.isEmpty() ? null : stack.toBukkit();
    }

    public static org.bukkit.inventory.ItemStack asCraftCopy(org.bukkit.inventory.ItemStack stack) {
        return stack == null ? null : stack.clone();
    }

    /**
     * A mirror must write through to a live server-side stack. Ferrite's first NMS slice keeps
     * NMS stacks as values, so pretending to support a mirror would silently lose writes.
     */
    public static org.bukkit.inventory.ItemStack asCraftMirror(ItemStack stack) {
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.inventory.CraftItemStack",
                "asCraftMirror",
                "mirror semantics need a live server-side item stack; use asBukkitCopy in the first slice");
    }

    /** Overload kept for plugins compiled against CraftBukkit's bukkit-to-bukkit mirror helper. */
    public static org.bukkit.inventory.ItemStack asCraftMirror(org.bukkit.inventory.ItemStack stack) {
        Objects.requireNonNull(stack, "stack");
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.inventory.CraftItemStack",
                "asCraftMirror",
                "mirror semantics need a live server-side item stack; use asCraftCopy in the first slice");
    }
}
