package dev.ferrite.api.entity;

import dev.ferrite.api.inventory.PlayerInventorySnapshot;
import dev.ferrite.api.inventory.UnsupportedPlayerInventoryBase;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.function.Consumer;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.entity.HumanEntity;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;

/**
 * Player inventory: 36 storage slots, 4 armor slots and the off hand (41 total).
 *
 * <p>Every read and write uses defensive {@link ItemStack} copies so mutating a caller's
 * stack cannot silently mutate stored inventory state. Mutations enforce the owning
 * server/region main-thread rule without synchronously waiting on another thread, and
 * {@link PlayerInventorySnapshot} provides the immutable boundary for host snapshots.
 *
 * <p>Holder-based menus, viewers and inventory events are outside the slice and inherit
 * explicit refusals from {@link UnsupportedPlayerInventoryBase}.
 */
public final class FerritePlayerInventory extends UnsupportedPlayerInventoryBase {

    private static final int MAIN_SLOTS = 36;
    private static final int ARMOR_START = 36;
    private static final int OFF_HAND = 40;

    private final FerritePlayer player;
    private final ItemStack[] contents = new ItemStack[PlayerInventorySnapshot.SIZE];
    private int heldSlot;
    private Consumer<PlayerInventorySnapshot> snapshotSink;

    public FerritePlayerInventory(FerritePlayer player) {
        this.player = player;
    }

    /** Attach a host snapshot sink notified after inventory mutations on the main thread. */
    public void setSnapshotSink(Consumer<PlayerInventorySnapshot> snapshotSink) {
        this.snapshotSink = snapshotSink;
    }

    /** Capture an immutable defensive snapshot of all 41 slots and the held slot. */
    public PlayerInventorySnapshot snapshot() {
        return PlayerInventorySnapshot.capture(contents, heldSlot);
    }

    /** Replace inventory state from an immutable host snapshot (main thread only). */
    public void applySnapshot(PlayerInventorySnapshot snapshot) {
        Objects.requireNonNull(snapshot, "snapshot");
        requirePrimaryThread("applySnapshot");
        ItemStack[] incoming = snapshot.contents();
        System.arraycopy(incoming, 0, contents, 0, contents.length);
        this.heldSlot = snapshot.heldSlot();
        publishSnapshot();
    }

    /**
     * Apply a host-authored snapshot without echoing it back through the snapshot sink.
     * Used for the initial join snapshot and host-side refreshes.
     */
    public void applyHostSnapshot(PlayerInventorySnapshot snapshot) {
        Objects.requireNonNull(snapshot, "snapshot");
        requirePrimaryThread("applyHostSnapshot");
        ItemStack[] incoming = snapshot.contents();
        System.arraycopy(incoming, 0, contents, 0, contents.length);
        this.heldSlot = snapshot.heldSlot();
    }

    /** Serialize the current inventory snapshot for the host RPC/snapshot boundary. */
    public Map<String, Object> serializeSnapshot() {
        return snapshot().serialize();
    }

    /** Restore inventory state from a serialized host snapshot (main thread only). */
    public void restoreSnapshot(Map<String, Object> serialized) {
        applySnapshot(PlayerInventorySnapshot.deserialize(serialized));
    }

    @Override
    public int getSize() {
        return contents.length;
    }

    @Override
    public ItemStack getItem(int index) {
        checkIndex(index);
        return copyOrNull(contents[index]);
    }

    @Override
    public void setItem(int index, ItemStack item) {
        checkIndex(index);
        requirePrimaryThread("setItem");
        contents[index] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack[] getContents() {
        return copyRange(0, contents.length);
    }

    @Override
    public void setContents(ItemStack[] items) {
        if (items != null && items.length > contents.length) {
            throw new IllegalArgumentException(
                    "A player inventory has " + contents.length + " slots, got " + items.length);
        }
        requirePrimaryThread("setContents");
        Arrays.fill(contents, null);
        if (items != null) {
            for (int index = 0; index < items.length; index++) {
                contents[index] = copyOrNull(items[index]);
            }
        }
        publishSnapshot();
    }

    @Override
    public ItemStack[] getStorageContents() {
        return copyRange(0, MAIN_SLOTS);
    }

    @Override
    public void setStorageContents(ItemStack[] items) {
        if (items != null && items.length > MAIN_SLOTS) {
            throw new IllegalArgumentException(
                    "A player inventory has " + MAIN_SLOTS + " storage slots, got " + items.length);
        }
        requirePrimaryThread("setStorageContents");
        Arrays.fill(contents, 0, MAIN_SLOTS, null);
        if (items != null) {
            for (int index = 0; index < items.length; index++) {
                contents[index] = copyOrNull(items[index]);
            }
        }
        publishSnapshot();
    }

    @Override
    public HashMap<Integer, ItemStack> addItem(ItemStack... items) {
        Objects.requireNonNull(items, "items");
        for (ItemStack item : items) {
            if (item == null) {
                throw new IllegalArgumentException("Item cannot be null");
            }
        }
        requirePrimaryThread("addItem");
        HashMap<Integer, ItemStack> leftover = new HashMap<>();
        boolean modified = false;

        for (int index = 0; index < items.length; index++) {
            ItemStack item = items[index];
            if (item.getType() == Material.AIR || item.getAmount() <= 0) {
                continue;
            }
            int remaining = item.getAmount();
            int maxStack = effectiveMaxStackSize(item);

            // First pass: top up existing partial stacks in storage slots (0..35).
            for (int slot = 0; slot < MAIN_SLOTS && remaining > 0; slot++) {
                ItemStack existing = contents[slot];
                if (existing != null && existing.isSimilar(item) && existing.getAmount() < maxStack) {
                    int space = maxStack - existing.getAmount();
                    int transfer = Math.min(remaining, space);
                    existing.setAmount(existing.getAmount() + transfer);
                    remaining -= transfer;
                    modified = true;
                }
            }

            // Second pass: place remaining items into empty storage slots.
            for (int slot = 0; slot < MAIN_SLOTS && remaining > 0; slot++) {
                if (contents[slot] == null) {
                    int transfer = Math.min(remaining, maxStack);
                    ItemStack placed = item.clone();
                    placed.setAmount(transfer);
                    contents[slot] = placed;
                    remaining -= transfer;
                    modified = true;
                }
            }

            if (remaining > 0) {
                ItemStack overflow = item.clone();
                overflow.setAmount(remaining);
                leftover.put(index, overflow);
            }
        }

        if (modified) {
            publishSnapshot();
        }
        return leftover;
    }

    @Override
    public HashMap<Integer, ItemStack> removeItem(ItemStack... items) {
        return removeMatching(MAIN_SLOTS, "removeItem", items);
    }

    @Override
    public HashMap<Integer, ItemStack> removeItemAnySlot(ItemStack... items) {
        return removeMatching(contents.length, "removeItemAnySlot", items);
    }

    private HashMap<Integer, ItemStack> removeMatching(
            int slotLimit, String operation, ItemStack... items) {
        Objects.requireNonNull(items, "items");
        for (ItemStack item : items) {
            if (item == null) {
                throw new IllegalArgumentException("Item cannot be null");
            }
        }
        requirePrimaryThread(operation);
        HashMap<Integer, ItemStack> leftover = new HashMap<>();
        boolean modified = false;

        for (int index = 0; index < items.length; index++) {
            ItemStack item = items[index];
            int toDelete = item.getAmount();
            if (item.getType() == Material.AIR || toDelete <= 0) {
                continue;
            }
            for (int slot = 0; slot < slotLimit && toDelete > 0; slot++) {
                ItemStack existing = contents[slot];
                if (existing != null && existing.isSimilar(item)) {
                    int existingAmount = existing.getAmount();
                    if (existingAmount <= toDelete) {
                        contents[slot] = null;
                        toDelete -= existingAmount;
                    } else {
                        existing.setAmount(existingAmount - toDelete);
                        toDelete = 0;
                    }
                    modified = true;
                }
            }
            if (toDelete > 0) {
                ItemStack remaining = item.clone();
                remaining.setAmount(toDelete);
                leftover.put(index, remaining);
            }
        }

        if (modified) {
            publishSnapshot();
        }
        return leftover;
    }

    @Override
    public void clear() {
        requirePrimaryThread("clear");
        Arrays.fill(contents, null);
        publishSnapshot();
    }

    @Override
    public void clear(int index) {
        checkIndex(index);
        requirePrimaryThread("clear");
        contents[index] = null;
        publishSnapshot();
    }

    @Override
    public boolean isEmpty() {
        return Arrays.stream(contents)
                .allMatch(item -> item == null || item.getType() == Material.AIR || item.getAmount() <= 0);
    }

    @Override
    public boolean contains(Material material) {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
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
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        if (amount <= 0) {
            return true;
        }
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
        if (amount <= 0) {
            return true;
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
    public HashMap<Integer, ItemStack> all(Material material) {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        HashMap<Integer, ItemStack> found = new HashMap<>();
        for (int slot = 0; slot < contents.length; slot++) {
            ItemStack item = contents[slot];
            if (item != null && item.getType() == material) {
                found.put(slot, item.clone());
            }
        }
        return found;
    }

    @Override
    public HashMap<Integer, ItemStack> all(ItemStack item) {
        HashMap<Integer, ItemStack> found = new HashMap<>();
        if (item == null) {
            return found;
        }
        for (int slot = 0; slot < contents.length; slot++) {
            ItemStack candidate = contents[slot];
            if (candidate != null && candidate.equals(item)) {
                found.put(slot, candidate.clone());
            }
        }
        return found;
    }

    @Override
    public int first(Material material) {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        for (int slot = 0; slot < contents.length; slot++) {
            ItemStack item = contents[slot];
            if (item != null && item.getType() == material) {
                return slot;
            }
        }
        return -1;
    }

    @Override
    public int first(ItemStack item) {
        if (item == null) {
            return -1;
        }
        for (int slot = 0; slot < contents.length; slot++) {
            ItemStack candidate = contents[slot];
            if (candidate != null && candidate.equals(item)) {
                return slot;
            }
        }
        return -1;
    }

    @Override
    public int firstEmpty() {
        for (int slot = 0; slot < MAIN_SLOTS; slot++) {
            if (contents[slot] == null) {
                return slot;
            }
        }
        return -1;
    }

    @Override
    public void remove(Material material) {
        if (material == null) {
            throw new IllegalArgumentException("Material cannot be null");
        }
        requirePrimaryThread("remove");
        boolean modified = false;
        for (int slot = 0; slot < contents.length; slot++) {
            if (contents[slot] != null && contents[slot].getType() == material) {
                contents[slot] = null;
                modified = true;
            }
        }
        if (modified) {
            publishSnapshot();
        }
    }

    @Override
    public void remove(ItemStack item) {
        requirePrimaryThread("remove");
        if (item == null) {
            return;
        }
        boolean modified = false;
        for (int slot = 0; slot < contents.length; slot++) {
            if (contents[slot] != null && contents[slot].equals(item)) {
                contents[slot] = null;
                modified = true;
            }
        }
        if (modified) {
            publishSnapshot();
        }
    }

    @Override
    public int getMaxStackSize() {
        return 64;
    }

    @Override
    public ItemStack getItemInMainHand() {
        return copyOrNull(contents[heldSlot]);
    }

    @Override
    public void setItemInMainHand(ItemStack item) {
        requirePrimaryThread("setItemInMainHand");
        contents[heldSlot] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack getItemInOffHand() {
        return copyOrNull(contents[OFF_HAND]);
    }

    @Override
    public void setItemInOffHand(ItemStack item) {
        requirePrimaryThread("setItemInOffHand");
        contents[OFF_HAND] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    @SuppressWarnings("deprecation")
    public ItemStack getItemInHand() {
        return getItemInMainHand();
    }

    @Override
    @SuppressWarnings("deprecation")
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
        requirePrimaryThread("setHeldItemSlot");
        this.heldSlot = slot;
        publishSnapshot();
    }

    @Override
    public ItemStack[] getArmorContents() {
        return new ItemStack[] {
            copyOrNull(contents[ARMOR_START + 3]),
            copyOrNull(contents[ARMOR_START + 2]),
            copyOrNull(contents[ARMOR_START + 1]),
            copyOrNull(contents[ARMOR_START])
        };
    }

    @Override
    public void setArmorContents(ItemStack[] items) {
        if (items != null && items.length > 4) {
            throw new IllegalArgumentException("Armor contents has 4 slots, got " + items.length);
        }
        requirePrimaryThread("setArmorContents");
        Arrays.fill(contents, ARMOR_START, ARMOR_START + 4, null);
        if (items != null) {
            for (int index = 0; index < items.length; index++) {
                contents[ARMOR_START + 3 - index] = copyOrNull(items[index]);
            }
        }
        publishSnapshot();
    }

    @Override
    public ItemStack[] getExtraContents() {
        return new ItemStack[] {copyOrNull(contents[OFF_HAND])};
    }

    @Override
    public void setExtraContents(ItemStack[] items) {
        if (items != null && items.length > 1) {
            throw new IllegalArgumentException("Extra contents has 1 slot, got " + items.length);
        }
        requirePrimaryThread("setExtraContents");
        contents[OFF_HAND] = items == null || items.length == 0 ? null : copyOrNull(items[0]);
        publishSnapshot();
    }

    @Override
    public ItemStack getHelmet() {
        return copyOrNull(contents[ARMOR_START + 3]);
    }

    @Override
    public void setHelmet(ItemStack item) {
        requirePrimaryThread("setHelmet");
        contents[ARMOR_START + 3] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack getChestplate() {
        return copyOrNull(contents[ARMOR_START + 2]);
    }

    @Override
    public void setChestplate(ItemStack item) {
        requirePrimaryThread("setChestplate");
        contents[ARMOR_START + 2] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack getLeggings() {
        return copyOrNull(contents[ARMOR_START + 1]);
    }

    @Override
    public void setLeggings(ItemStack item) {
        requirePrimaryThread("setLeggings");
        contents[ARMOR_START + 1] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack getBoots() {
        return copyOrNull(contents[ARMOR_START]);
    }

    @Override
    public void setBoots(ItemStack item) {
        requirePrimaryThread("setBoots");
        contents[ARMOR_START] = copyOrNull(item);
        publishSnapshot();
    }

    @Override
    public ItemStack getItem(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
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
        Objects.requireNonNull(slot, "slot");
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
        return player == null ? null : player.getLocation();
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

    private int effectiveMaxStackSize(ItemStack item) {
        int itemMax = item.getMaxStackSize();
        if (itemMax <= 0) {
            return getMaxStackSize();
        }
        return Math.min(getMaxStackSize(), itemMax);
    }

    private ItemStack[] copyRange(int start, int length) {
        ItemStack[] copy = new ItemStack[length];
        for (int index = 0; index < length; index++) {
            copy[index] = copyOrNull(contents[start + index]);
        }
        return copy;
    }

    private static ItemStack copyOrNull(ItemStack item) {
        if (item == null || item.getType() == Material.AIR || item.getAmount() <= 0) {
            return null;
        }
        return item.clone();
    }

    private void checkIndex(int index) {
        if (index < 0 || index >= contents.length) {
            throw new ArrayIndexOutOfBoundsException(
                    "slot " + index + " is outside 0.." + (contents.length - 1));
        }
    }

    private void requirePrimaryThread(String operation) {
        if (player != null && player.getServer() != null && !player.getServer().isPrimaryThread()) {
            throw new IllegalStateException(
                    "PlayerInventory#"
                            + operation
                            + " must run on the Ferrite server main thread; current thread is '"
                            + Thread.currentThread().getName()
                            + "'");
        }
    }

    private void publishSnapshot() {
        if (snapshotSink != null) {
            snapshotSink.accept(snapshot());
        }
    }
}
