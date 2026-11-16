package dev.ferrite.api.entity;

import dev.ferrite.api.inventory.UnsupportedPlayerInventoryBase;
import java.util.Arrays;
import java.util.List;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.entity.HumanEntity;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;

/**
 * Basic player inventory: 36 main slots, 4 armor slots and the off hand.
 *
 * <p>Only slot reads/writes are implemented. Holder-based menus, viewers and inventory
 * events are outside the first slice and stay explicit refusals.
 */
public final class FerritePlayerInventory extends UnsupportedPlayerInventoryBase {

    private static final int MAIN_SLOTS = 36;
    private static final int ARMOR_START = 36;
    private static final int OFF_HAND = 40;

    private final FerritePlayer player;
    private final ItemStack[] contents = new ItemStack[41];
    private int heldSlot;

    public FerritePlayerInventory(FerritePlayer player) {
        this.player = player;
    }

    @Override
    public int getSize() {
        return contents.length;
    }

    @Override
    public ItemStack getItem(int index) {
        checkIndex(index);
        return contents[index];
    }

    @Override
    public void setItem(int index, ItemStack item) {
        checkIndex(index);
        contents[index] = item;
    }

    @Override
    public ItemStack[] getContents() {
        return contents.clone();
    }

    @Override
    public void setContents(ItemStack[] items) {
        if (items != null && items.length > MAIN_SLOTS) {
            throw new IllegalArgumentException(
                    "A player inventory has " + MAIN_SLOTS + " main slots, got " + items.length);
        }
        Arrays.fill(contents, 0, MAIN_SLOTS, null);
        if (items != null) {
            System.arraycopy(items, 0, contents, 0, items.length);
        }
    }

    @Override
    public void clear() {
        Arrays.fill(contents, null);
    }

    @Override
    public void clear(int index) {
        checkIndex(index);
        contents[index] = null;
    }

    @Override
    public boolean isEmpty() {
        return Arrays.stream(contents).allMatch(item -> item == null || item.getType() == Material.AIR);
    }

    @Override
    public boolean contains(Material material) {
        for (ItemStack item : contents) {
            if (item != null && item.getType() == material) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean contains(ItemStack item) {
        return item != null && containsAtLeast(item, 1);
    }

    @Override
    public boolean contains(Material material, int amount) {
        return count(material) >= amount;
    }

    @Override
    public boolean contains(ItemStack item, int amount) {
        return item != null && containsAtLeast(item, amount);
    }

    @Override
    public boolean containsAtLeast(ItemStack item, int amount) {
        if (item == null) {
            return false;
        }
        int found = 0;
        for (ItemStack candidate : contents) {
            if (candidate != null && candidate.isSimilar(item)) {
                found += candidate.getAmount();
            }
        }
        return found >= amount;
    }

    @Override
    public int getMaxStackSize() {
        return 64;
    }

    @Override
    public ItemStack getItemInMainHand() {
        return contents[heldSlot];
    }

    @Override
    public void setItemInMainHand(ItemStack item) {
        contents[heldSlot] = item;
    }

    @Override
    public ItemStack getItemInOffHand() {
        return contents[OFF_HAND];
    }

    @Override
    public void setItemInOffHand(ItemStack item) {
        contents[OFF_HAND] = item;
    }

    @Override
    public ItemStack getItemInHand() {
        return getItemInMainHand();
    }

    @Override
    public void setItemInHand(ItemStack item) {
        setItemInMainHand(item);
    }

    @Override
    public int getHeldItemSlot() {
        return heldSlot;
    }

    @Override
    public void setHeldItemSlot(int slot) {
        if (slot < 0 || slot > 8) {
            throw new IllegalArgumentException("held slot must be between 0 and 8, was " + slot);
        }
        this.heldSlot = slot;
    }

    @Override
    public ItemStack[] getArmorContents() {
        return new ItemStack[] {
            contents[ARMOR_START + 3], contents[ARMOR_START + 2], contents[ARMOR_START + 1], contents[ARMOR_START]
        };
    }

    @Override
    public void setArmorContents(ItemStack[] items) {
        if (items == null) {
            Arrays.fill(contents, ARMOR_START, ARMOR_START + 4, null);
            return;
        }
        if (items.length > 4) {
            throw new IllegalArgumentException("Armor contents has 4 slots, got " + items.length);
        }
        Arrays.fill(contents, ARMOR_START, ARMOR_START + 4, null);
        for (int index = 0; index < items.length; index++) {
            contents[ARMOR_START + 3 - index] = items[index];
        }
    }

    @Override
    public ItemStack[] getExtraContents() {
        return new ItemStack[] {contents[OFF_HAND]};
    }

    @Override
    public void setExtraContents(ItemStack[] items) {
        contents[OFF_HAND] = items == null || items.length == 0 ? null : items[0];
    }

    @Override
    public ItemStack getHelmet() {
        return contents[ARMOR_START + 3];
    }

    @Override
    public void setHelmet(ItemStack item) {
        contents[ARMOR_START + 3] = item;
    }

    @Override
    public ItemStack getChestplate() {
        return contents[ARMOR_START + 2];
    }

    @Override
    public void setChestplate(ItemStack item) {
        contents[ARMOR_START + 2] = item;
    }

    @Override
    public ItemStack getLeggings() {
        return contents[ARMOR_START + 1];
    }

    @Override
    public void setLeggings(ItemStack item) {
        contents[ARMOR_START + 1] = item;
    }

    @Override
    public ItemStack getBoots() {
        return contents[ARMOR_START];
    }

    @Override
    public void setBoots(ItemStack item) {
        contents[ARMOR_START] = item;
    }

    @Override
    public ItemStack getItem(EquipmentSlot slot) {
        return switch (slot) {
            case HAND -> getItemInMainHand();
            case OFF_HAND -> getItemInOffHand();
            case HEAD -> getHelmet();
            case CHEST -> getChestplate();
            case LEGS -> getLeggings();
            case FEET -> getBoots();
            default -> throw new IllegalArgumentException("Unsupported equipment slot: " + slot);
        };
    }

    @Override
    public void setItem(EquipmentSlot slot, ItemStack item) {
        switch (slot) {
            case HAND -> setItemInMainHand(item);
            case OFF_HAND -> setItemInOffHand(item);
            case HEAD -> setHelmet(item);
            case CHEST -> setChestplate(item);
            case LEGS -> setLeggings(item);
            case FEET -> setBoots(item);
            default -> throw new IllegalArgumentException("Unsupported equipment slot: " + slot);
        }
    }

    @Override
    public HumanEntity getHolder() {
        return player;
    }

    @Override
    public Location getLocation() {
        return player.getLocation();
    }

    @Override
    public InventoryType getType() {
        return InventoryType.PLAYER;
    }

    @Override
    public List<HumanEntity> getViewers() {
        return List.of();
    }

    private int count(Material material) {
        int found = 0;
        for (ItemStack item : contents) {
            if (item != null && item.getType() == material) {
                found += item.getAmount();
            }
        }
        return found;
    }

    private void checkIndex(int index) {
        if (index < 0 || index >= contents.length) {
            throw new ArrayIndexOutOfBoundsException(
                    "slot " + index + " is outside 0.." + (contents.length - 1));
        }
    }
}
