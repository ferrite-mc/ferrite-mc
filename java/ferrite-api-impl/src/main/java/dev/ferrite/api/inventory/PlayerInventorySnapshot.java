package dev.ferrite.api.inventory;

import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;

/**
 * Immutable defensive snapshot of a player's 41-slot inventory and selected hotbar slot.
 *
 * <p>Used as the serialization and copy boundary between the Bukkit facade and the Ferrite
 * host. Every {@link ItemStack} captured into or read from a snapshot is cloned so neither
 * side can mutate the other's state.
 */
public final class PlayerInventorySnapshot {

    public static final int SIZE = 41;
    public static final int MAIN_SIZE = 36;

    private final ItemStack[] slots;
    private final int heldSlot;

    private PlayerInventorySnapshot(ItemStack[] slots, int heldSlot) {
        this.slots = slots;
        this.heldSlot = heldSlot;
    }

    public static PlayerInventorySnapshot capture(ItemStack[] contents, int heldSlot) {
        Objects.requireNonNull(contents, "contents");
        if (contents.length != SIZE) {
            throw new IllegalArgumentException(
                    "PlayerInventorySnapshot requires " + SIZE + " slots, got " + contents.length);
        }
        checkHeldSlot(heldSlot);
        ItemStack[] copy = new ItemStack[SIZE];
        for (int i = 0; i < SIZE; i++) {
            copy[i] = copyOrNull(contents[i]);
        }
        return new PlayerInventorySnapshot(copy, heldSlot);
    }

    public int size() {
        return SIZE;
    }

    public int heldSlot() {
        return heldSlot;
    }

    public ItemStack item(int index) {
        if (index < 0 || index >= SIZE) {
            throw new ArrayIndexOutOfBoundsException(
                    "slot " + index + " is outside 0.." + (SIZE - 1));
        }
        return copyOrNull(slots[index]);
    }

    public ItemStack[] contents() {
        ItemStack[] copy = new ItemStack[SIZE];
        for (int i = 0; i < SIZE; i++) {
            copy[i] = copyOrNull(slots[i]);
        }
        return copy;
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("held-slot", heldSlot);
        Map<String, Object> serializedSlots = new LinkedHashMap<>();
        for (int i = 0; i < SIZE; i++) {
            ItemStack item = slots[i];
            if (item != null && item.getType() != Material.AIR && item.getAmount() > 0) {
                serializedSlots.put(Integer.toString(i), item.serialize());
            }
        }
        result.put("slots", serializedSlots);
        return result;
    }

    @SuppressWarnings("unchecked")
    public static PlayerInventorySnapshot deserialize(Map<String, Object> args) {
        Objects.requireNonNull(args, "args");
        int heldSlot = 0;
        Object rawHeld = args.get("held-slot");
        if (rawHeld instanceof Number number) {
            heldSlot = number.intValue();
        }
        checkHeldSlot(heldSlot);

        ItemStack[] restored = new ItemStack[SIZE];
        Object rawSlots = args.get("slots");
        if (rawSlots instanceof Map<?, ?> slotMap) {
            for (Map.Entry<?, ?> entry : slotMap.entrySet()) {
                int slot = Integer.parseInt(String.valueOf(entry.getKey()));
                if (slot < 0 || slot >= SIZE) {
                    throw new ArrayIndexOutOfBoundsException(
                            "slot " + slot + " is outside 0.." + (SIZE - 1));
                }
                if (entry.getValue() instanceof Map<?, ?> itemMap) {
                    restored[slot] = copyOrNull(ItemStack.deserialize((Map<String, Object>) itemMap));
                } else if (entry.getValue() != null) {
                    throw new IllegalArgumentException(
                            "Expected serialized item map for slot " + slot);
                }
            }
        }
        return new PlayerInventorySnapshot(restored, heldSlot);
    }

    private static void checkHeldSlot(int heldSlot) {
        if (heldSlot < 0 || heldSlot > 8) {
            throw new IllegalArgumentException(
                    "held slot must be between 0 and 8, was " + heldSlot);
        }
    }

    private static ItemStack copyOrNull(ItemStack item) {
        if (item == null || item.getType() == Material.AIR || item.getAmount() <= 0) {
            return null;
        }
        return item.clone();
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof PlayerInventorySnapshot other)) {
            return false;
        }
        return heldSlot == other.heldSlot && Arrays.equals(slots, other.slots);
    }

    @Override
    public int hashCode() {
        return 31 * heldSlot + Arrays.hashCode(slots);
    }
}
