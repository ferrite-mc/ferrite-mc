package dev.ferrite.server;

import java.util.Objects;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;

/**
 * `Player#getEquipment`: the worn armor and both hands as `EntityEquipment`.
 *
 * <p>It reads and writes the player's inventory, so every change goes through the host like any
 * other inventory change. Unlike the inventory itself, {@link #clear()} only empties the equipment
 * slots, a player drops everything on death (drop chance 1) and the chance cannot be changed
 * (`CraftEntityEquipment` only keeps drop chances for a `Mob`), and the body and saddle slots do not
 * exist on a player.
 */
final class FerriteEntityEquipment implements EntityEquipment {
    private static final EquipmentSlot[] SLOTS = {EquipmentSlot.HAND, EquipmentSlot.OFF_HAND, EquipmentSlot.FEET, EquipmentSlot.LEGS, EquipmentSlot.CHEST, EquipmentSlot.HEAD};
    private final Player holder;

    FerriteEntityEquipment(Player holder) { this.holder = Objects.requireNonNull(holder, "holder"); }

    private PlayerInventory inventory() { return holder.getInventory(); }

    private static boolean wearable(EquipmentSlot slot) {
        return slot != EquipmentSlot.BODY && slot != EquipmentSlot.SADDLE;
    }

    private static boolean empty(ItemStack item) { return item == null || item.getType().isAir(); }

    @Override public void setItem(EquipmentSlot slot, ItemStack item) { setItem(slot, item, false); }

    /** The `silent` flag only suppresses the equip sound in vanilla; Ferrite plays none either way. */
    @Override public void setItem(EquipmentSlot slot, ItemStack item, boolean silent) {
        Objects.requireNonNull(slot, "slot");
        if (!wearable(slot)) {
            if (empty(item)) return;
            throw new IllegalArgumentException("A player has no " + slot + " slot");
        }
        inventory().setItem(slot, item);
    }

    @Override public ItemStack getItem(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        // `CraftEntityEquipment` never returns null for an empty slot: it is an air stack.
        var item = wearable(slot) ? inventory().getItem(slot) : null;
        return item == null ? new ItemStack(org.bukkit.Material.AIR) : item;
    }

    @Override public ItemStack getItemInMainHand() { return getItem(EquipmentSlot.HAND); }
    @Override public void setItemInMainHand(ItemStack item) { setItem(EquipmentSlot.HAND, item); }
    @Override public void setItemInMainHand(ItemStack item, boolean silent) { setItem(EquipmentSlot.HAND, item, silent); }
    @Override public ItemStack getItemInOffHand() { return getItem(EquipmentSlot.OFF_HAND); }
    @Override public void setItemInOffHand(ItemStack item) { setItem(EquipmentSlot.OFF_HAND, item); }
    @Override public void setItemInOffHand(ItemStack item, boolean silent) { setItem(EquipmentSlot.OFF_HAND, item, silent); }
    @Override public ItemStack getItemInHand() { return getItemInMainHand(); }
    @Override public void setItemInHand(ItemStack item) { setItemInMainHand(item); }
    @Override public ItemStack getHelmet() { return getItem(EquipmentSlot.HEAD); }
    @Override public void setHelmet(ItemStack item) { setItem(EquipmentSlot.HEAD, item); }
    @Override public void setHelmet(ItemStack item, boolean silent) { setItem(EquipmentSlot.HEAD, item, silent); }
    @Override public ItemStack getChestplate() { return getItem(EquipmentSlot.CHEST); }
    @Override public void setChestplate(ItemStack item) { setItem(EquipmentSlot.CHEST, item); }
    @Override public void setChestplate(ItemStack item, boolean silent) { setItem(EquipmentSlot.CHEST, item, silent); }
    @Override public ItemStack getLeggings() { return getItem(EquipmentSlot.LEGS); }
    @Override public void setLeggings(ItemStack item) { setItem(EquipmentSlot.LEGS, item); }
    @Override public void setLeggings(ItemStack item, boolean silent) { setItem(EquipmentSlot.LEGS, item, silent); }
    @Override public ItemStack getBoots() { return getItem(EquipmentSlot.FEET); }
    @Override public void setBoots(ItemStack item) { setItem(EquipmentSlot.FEET, item); }
    @Override public void setBoots(ItemStack item, boolean silent) { setItem(EquipmentSlot.FEET, item, silent); }

    private static final EquipmentSlot[] ARMOR = {EquipmentSlot.FEET, EquipmentSlot.LEGS, EquipmentSlot.CHEST, EquipmentSlot.HEAD};

    /** Boots, leggings, chestplate, helmet: index 0 is the boots, as in `PlayerInventory`. */
    @Override public ItemStack[] getArmorContents() {
        var out = new ItemStack[ARMOR.length];
        for (int i = 0; i < out.length; i++) out[i] = getItem(ARMOR[i]);
        return out;
    }

    /** Slots past the end of a shorter array are emptied; a longer array is refused (`setSlots`). */
    @Override public void setArmorContents(ItemStack[] items) {
        if (items != null && items.length > ARMOR.length) throw new IllegalArgumentException("items.length must be <= " + ARMOR.length);
        for (int i = 0; i < ARMOR.length; i++) setItem(ARMOR[i], items == null || i >= items.length ? null : items[i]);
    }

    /** Empties the armor and both hands only; the rest of the inventory stays. */
    @Override public void clear() {
        for (var slot : SLOTS) inventory().setItem(slot, null);
    }

    private static float always() { return 1.0f; }
    private static UnsupportedOperationException fixed() {
        return new UnsupportedOperationException("A player always drops its equipment; the drop chance only exists for mobs");
    }

    @Override public float getItemInHandDropChance() { return always(); }
    @Override public void setItemInHandDropChance(float chance) { throw fixed(); }
    @Override public float getItemInMainHandDropChance() { return always(); }
    @Override public void setItemInMainHandDropChance(float chance) { throw fixed(); }
    @Override public float getItemInOffHandDropChance() { return always(); }
    @Override public void setItemInOffHandDropChance(float chance) { throw fixed(); }
    @Override public float getHelmetDropChance() { return always(); }
    @Override public void setHelmetDropChance(float chance) { throw fixed(); }
    @Override public float getChestplateDropChance() { return always(); }
    @Override public void setChestplateDropChance(float chance) { throw fixed(); }
    @Override public float getLeggingsDropChance() { return always(); }
    @Override public void setLeggingsDropChance(float chance) { throw fixed(); }
    @Override public float getBootsDropChance() { return always(); }
    @Override public void setBootsDropChance(float chance) { throw fixed(); }
    @Override public float getDropChance(EquipmentSlot slot) { Objects.requireNonNull(slot, "slot"); return always(); }
    @Override public void setDropChance(EquipmentSlot slot, float chance) { Objects.requireNonNull(slot, "slot"); throw fixed(); }

    @Override public Entity getHolder() { return holder; }
}
