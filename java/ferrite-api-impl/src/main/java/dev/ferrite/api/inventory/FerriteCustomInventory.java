package dev.ferrite.api.inventory;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.Unsupported;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.ListIterator;
import java.util.NoSuchElementException;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.entity.HumanEntity;
import org.bukkit.event.inventory.InventoryCloseEvent;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryHolder;
import org.bukkit.inventory.ItemStack;

/**
 * Plugin-created container inventory supporting chest/barrel, furnace/blast furnace/smoker,
 * crafting, anvil, enchanting, merchant, hopper, dispenser/dropper, and shulker box types.
 *
 * <p>All reads/writes use defensive {@link ItemStack} copies and enforce the main-thread rule.
 */
public final class FerriteCustomInventory implements Inventory {

    private final FerriteRuntime runtime;
    private final InventoryHolder holder;
    private final InventoryType type;
    private final Component title;
    private final ItemStack[] contents;
    private final List<HumanEntity> viewers = new ArrayList<>();
    private int maxStackSize = 64;

    private FerriteCustomInventory(
            FerriteRuntime runtime,
            InventoryHolder holder,
            InventoryType type,
            int size,
            Component title) {
        this.runtime = Objects.requireNonNull(runtime, "runtime");
        this.holder = holder;
        this.type = Objects.requireNonNull(type, "type");
        this.title = title == null ? Component.text(type.getDefaultTitle()) : title;
        this.contents = new ItemStack[size];
    }

    /** Creates a chest-backed custom inventory with {@code size} slots (multiple of 9, 9..54). */
    public static FerriteCustomInventory createChest(
            FerriteRuntime runtime, InventoryHolder holder, int size, Component title) {
        if (size <= 0 || size % 9 != 0 || size > 54) {
            throw new IllegalArgumentException(
                    "A chest inventory size must be a positive multiple of 9 up to 54, got " + size);
        }
        return new FerriteCustomInventory(runtime, holder, InventoryType.CHEST, size, title);
    }

    /** Creates a typed custom inventory, or fails explicitly for unsupported container types. */
    public static FerriteCustomInventory createTyped(
            FerriteRuntime runtime, InventoryHolder holder, InventoryType type, Component title) {
        Objects.requireNonNull(type, "type");
        int size = switch (type) {
            case CHEST, BARREL, ENDER_CHEST, SHULKER_BOX -> 27;
            case FURNACE, BLAST_FURNACE, SMOKER, ANVIL, MERCHANT, CARTOGRAPHY, GRINDSTONE -> 3;
            case WORKBENCH, CRAFTING -> 10;
            case ENCHANTING, STONECUTTER -> 2;
            case LOOM, SMITHING -> 4;
            case HOPPER, BREWING -> 5;
            case DISPENSER, DROPPER, CRAFTER -> 9;
            case BEACON, LECTERN -> 1;
            default -> throw Unsupported.exception("Server#createInventory(" + type.name() + ")");
        };
        return new FerriteCustomInventory(runtime, holder, type, size, title);
    }

    public Component title() {
        return title;
    }

    public String legacyTitle() {
        return LegacyComponentSerializer.legacySection().serialize(title);
    }

    public void addViewer(HumanEntity viewer) {
        if (!viewers.contains(viewer)) {
            viewers.add(viewer);
        }
    }

    public void removeViewer(HumanEntity viewer) {
        viewers.remove(viewer);
    }

    @Override
    public int getSize() {
        return contents.length;
    }

    @Override
    public int getMaxStackSize() {
        return maxStackSize;
    }

    @Override
    public void setMaxStackSize(int size) {
        if (size <= 0 || size > 99) {
            throw new IllegalArgumentException("maxStackSize must be between 1 and 99, got " + size);
        }
        this.maxStackSize = size;
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
        for (int i = 0; i < items.length; i++) {
            ItemStack item = items[i];
            if (item.getType() == Material.AIR || item.getAmount() <= 0) {
                continue;
            }
            int remaining = item.getAmount();
            int cap = Math.min(maxStackSize, Math.max(1, item.getMaxStackSize()));
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                ItemStack existing = contents[slot];
                if (existing != null && existing.isSimilar(item) && existing.getAmount() < cap) {
                    int add = Math.min(remaining, cap - existing.getAmount());
                    existing.setAmount(existing.getAmount() + add);
                    remaining -= add;
                }
            }
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                if (contents[slot] == null) {
                    int add = Math.min(remaining, cap);
                    ItemStack placed = item.clone();
                    placed.setAmount(add);
                    contents[slot] = placed;
                    remaining -= add;
                }
            }
            if (remaining > 0) {
                ItemStack rem = item.clone();
                rem.setAmount(remaining);
                leftover.put(i, rem);
            }
        }
        return leftover;
    }

    @Override
    public HashMap<Integer, ItemStack> removeItem(ItemStack... items) {
        Objects.requireNonNull(items, "items");
        for (ItemStack item : items) {
            if (item == null) {
                throw new IllegalArgumentException("Item cannot be null");
            }
        }
        requirePrimaryThread("removeItem");
        HashMap<Integer, ItemStack> missing = new HashMap<>();
        for (int i = 0; i < items.length; i++) {
            ItemStack item = items[i];
            if (item.getType() == Material.AIR || item.getAmount() <= 0) {
                continue;
            }
            int remaining = item.getAmount();
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                ItemStack existing = contents[slot];
                if (existing != null && existing.isSimilar(item)) {
                    int take = Math.min(remaining, existing.getAmount());
                    int left = existing.getAmount() - take;
                    contents[slot] = left <= 0 ? null : withAmount(existing, left);
                    remaining -= take;
                }
            }
            if (remaining > 0) {
                ItemStack rem = item.clone();
                rem.setAmount(remaining);
                missing.put(i, rem);
            }
        }
        return missing;
    }

    @Override
    public HashMap<Integer, ItemStack> removeItemAnySlot(ItemStack... items) {
        return removeItem(items);
    }

    @Override
    public ItemStack[] getContents() {
        ItemStack[] copy = new ItemStack[contents.length];
        for (int i = 0; i < contents.length; i++) {
            copy[i] = copyOrNull(contents[i]);
        }
        return copy;
    }

    @Override
    public void setContents(ItemStack[] items) {
        if (items != null && items.length > contents.length) {
            throw new IllegalArgumentException(
                    "Inventory has " + contents.length + " slots, got " + items.length);
        }
        requirePrimaryThread("setContents");
        Arrays.fill(contents, null);
        if (items != null) {
            for (int i = 0; i < items.length; i++) {
                contents[i] = copyOrNull(items[i]);
            }
        }
    }

    @Override
    public ItemStack[] getStorageContents() {
        return getContents();
    }

    @Override
    public void setStorageContents(ItemStack[] items) {
        setContents(items);
    }

    @Override
    public boolean contains(Material material) {
        Objects.requireNonNull(material, "material");
        return first(material) >= 0;
    }

    @Override
    public boolean contains(ItemStack item) {
        return item != null && first(item) >= 0;
    }

    @Override
    public boolean contains(Material material, int amount) {
        Objects.requireNonNull(material, "material");
        if (amount <= 0) {
            return true;
        }
        int found = 0;
        for (ItemStack stack : contents) {
            if (stack != null && stack.getType() == material) {
                found += stack.getAmount();
                if (found >= amount) {
                    return true;
                }
            }
        }
        return false;
    }

    @Override
    public boolean contains(ItemStack item, int amount) {
        if (item == null) {
            return false;
        }
        if (amount <= 0) {
            return true;
        }
        int found = 0;
        for (ItemStack stack : contents) {
            if (stack != null && stack.equals(item)) {
                found++;
                if (found >= amount) {
                    return true;
                }
            }
        }
        return false;
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
        for (ItemStack stack : contents) {
            if (stack != null && stack.isSimilar(item)) {
                found += stack.getAmount();
                if (found >= amount) {
                    return true;
                }
            }
        }
        return false;
    }

    @Override
    public HashMap<Integer, ItemStack> all(Material material) {
        Objects.requireNonNull(material, "material");
        HashMap<Integer, ItemStack> map = new HashMap<>();
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].getType() == material) {
                map.put(i, copyOrNull(contents[i]));
            }
        }
        return map;
    }

    @Override
    public HashMap<Integer, ItemStack> all(ItemStack item) {
        HashMap<Integer, ItemStack> map = new HashMap<>();
        if (item == null) {
            return map;
        }
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].equals(item)) {
                map.put(i, copyOrNull(contents[i]));
            }
        }
        return map;
    }

    @Override
    public int first(Material material) {
        Objects.requireNonNull(material, "material");
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].getType() == material) {
                return i;
            }
        }
        return -1;
    }

    @Override
    public int first(ItemStack item) {
        if (item == null) {
            return -1;
        }
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].equals(item)) {
                return i;
            }
        }
        return -1;
    }

    @Override
    public int firstEmpty() {
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] == null) {
                return i;
            }
        }
        return -1;
    }

    @Override
    public boolean isEmpty() {
        for (ItemStack stack : contents) {
            if (stack != null && stack.getType() != Material.AIR && stack.getAmount() > 0) {
                return false;
            }
        }
        return true;
    }

    @Override
    public void remove(Material material) {
        Objects.requireNonNull(material, "material");
        requirePrimaryThread("remove");
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].getType() == material) {
                contents[i] = null;
            }
        }
    }

    @Override
    public void remove(ItemStack item) {
        if (item == null) {
            return;
        }
        requirePrimaryThread("remove");
        for (int i = 0; i < contents.length; i++) {
            if (contents[i] != null && contents[i].equals(item)) {
                contents[i] = null;
            }
        }
    }

    @Override
    public void clear(int index) {
        checkIndex(index);
        requirePrimaryThread("clear");
        contents[index] = null;
    }

    @Override
    public void clear() {
        requirePrimaryThread("clear");
        Arrays.fill(contents, null);
    }

    @Override
    public int close() {
        requirePrimaryThread("close");
        List<HumanEntity> snapshot = List.copyOf(viewers);
        for (HumanEntity viewer : snapshot) {
            viewer.closeInventory(InventoryCloseEvent.Reason.PLUGIN);
        }
        return snapshot.size();
    }

    @Override
    public List<HumanEntity> getViewers() {
        return List.copyOf(viewers);
    }

    @Override
    public InventoryType getType() {
        return type;
    }

    @Override
    public InventoryHolder getHolder() {
        return holder;
    }

    @Override
    public InventoryHolder getHolder(boolean useSnapshot) {
        return holder;
    }

    @Override
    public ListIterator<ItemStack> iterator() {
        return iterator(0);
    }

    @Override
    public ListIterator<ItemStack> iterator(int index) {
        if (index < 0 || index > contents.length) {
            throw new IndexOutOfBoundsException("Index " + index + " out of bounds for " + contents.length);
        }
        return new ListIterator<>() {
            private int cursor = index;

            @Override
            public boolean hasNext() {
                return cursor < contents.length;
            }

            @Override
            public ItemStack next() {
                if (!hasNext()) {
                    throw new NoSuchElementException();
                }
                return copyOrNull(contents[cursor++]);
            }

            @Override
            public boolean hasPrevious() {
                return cursor > 0;
            }

            @Override
            public ItemStack previous() {
                if (!hasPrevious()) {
                    throw new NoSuchElementException();
                }
                return copyOrNull(contents[--cursor]);
            }

            @Override
            public int nextIndex() {
                return cursor;
            }

            @Override
            public int previousIndex() {
                return cursor - 1;
            }

            @Override
            public void remove() {
                throw new UnsupportedOperationException("Inventory iterator does not support remove");
            }

            @Override
            public void set(ItemStack item) {
                throw new UnsupportedOperationException("Inventory iterator does not support set");
            }

            @Override
            public void add(ItemStack item) {
                throw new UnsupportedOperationException("Inventory iterator does not support add");
            }
        };
    }

    @Override
    public Location getLocation() {
        return null;
    }

    private void checkIndex(int index) {
        if (index < 0 || index >= contents.length) {
            throw new IndexOutOfBoundsException(
                    "Slot " + index + " out of bounds for size " + contents.length);
        }
    }

    private void requirePrimaryThread(String operation) {
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException(
                    "Inventory#" + operation + " must run on the owning server/region thread");
        }
    }

    private static ItemStack copyOrNull(ItemStack stack) {
        if (stack == null || stack.getType() == Material.AIR || stack.getAmount() <= 0) {
            return null;
        }
        return stack.clone();
    }

    private static ItemStack withAmount(ItemStack stack, int amount) {
        ItemStack copy = stack.clone();
        copy.setAmount(amount);
        return copy;
    }
}
