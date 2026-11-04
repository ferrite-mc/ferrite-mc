package dev.ferrite.server;

/** In-memory inventory for plugin-created containers, independent of world inventories. */
final class FerriteInventory implements java.lang.reflect.InvocationHandler {
    private final org.bukkit.inventory.InventoryHolder holder;
    private final org.bukkit.event.inventory.InventoryType type;
    private final org.bukkit.inventory.ItemStack[] contents;
    private final org.bukkit.inventory.Inventory proxy;
    private int maxStack = 64;
    private String title = "";

    private FerriteInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, int size) {
        this.holder = holder; this.type = type; this.contents = new org.bukkit.inventory.ItemStack[size];
        this.proxy = (org.bukkit.inventory.Inventory) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.Inventory.class}, this);
    }
    static org.bukkit.inventory.Inventory create(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, int size) {
        if (size < 1 || (type == org.bukkit.event.inventory.InventoryType.CHEST && size % 9 != 0)) throw new IllegalArgumentException("Invalid inventory size: " + size);
        return new FerriteInventory(holder, type, size).proxy;
    }
    static org.bukkit.inventory.Inventory create(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, int size, String title) {
        var inventory = create(holder, type, size);
        ((FerriteInventory) java.lang.reflect.Proxy.getInvocationHandler(inventory)).title = title == null ? "" : title;
        return inventory;
    }
    /** The plain-text title the host shows when the inventory is opened. */
    static String titleOf(org.bukkit.inventory.Inventory inventory) {
        return isFerrite(inventory) ? ((FerriteInventory) java.lang.reflect.Proxy.getInvocationHandler(inventory)).title : "";
    }
    /** Wire form for the host: title, size, type, then one field per slot (see plugins/inventory.rs). */
    static String wire(org.bukkit.inventory.Inventory inventory) {
        var handler = (FerriteInventory) java.lang.reflect.Proxy.getInvocationHandler(inventory);
        StringBuilder out = new StringBuilder(handler.title).append('\u001e').append(handler.contents.length).append('\u001e').append(handler.type.name());
        for (var item : handler.contents) {
            out.append('\u001e');
            if (item != null && !isAir(item.getType()) && item.getAmount() > 0) out.append(item.getType().name()).append('\u001d').append(item.getAmount());
        }
        return out.toString();
    }
    static boolean isFerrite(Object value) {
        return value != null && java.lang.reflect.Proxy.isProxyClass(value.getClass()) && java.lang.reflect.Proxy.getInvocationHandler(value) instanceof FerriteInventory;
    }
    private static boolean isAir(org.bukkit.Material material) { return material == org.bukkit.Material.AIR || material == org.bukkit.Material.CAVE_AIR || material == org.bukkit.Material.VOID_AIR; }
    private boolean empty(org.bukkit.inventory.ItemStack item) { return item == null || isAir(item.getType()) || item.getAmount() <= 0; }
    private boolean matches(org.bukkit.inventory.ItemStack slot, Object wanted, boolean similar) {
        if (empty(slot)) return false;
        if (wanted instanceof org.bukkit.Material material) return slot.getType() == material;
        var item = (org.bukkit.inventory.ItemStack) wanted;
        return alike(slot, item) && (similar || slot.getAmount() == item.getAmount());
    }
    /** Same item type and meta; Paper's ItemStack#isSimilar needs a CraftItemStack delegate that plain stacks lack. */
    private static boolean alike(org.bukkit.inventory.ItemStack a, org.bukkit.inventory.ItemStack b) {
        if (a == b) return true;
        if (a == null || b == null || a.getType() != b.getType()) return false;
        return java.util.Objects.equals(a.hasItemMeta() ? a.getItemMeta() : null, b.hasItemMeta() ? b.getItemMeta() : null);
    }
    private int limit(org.bukkit.inventory.ItemStack item) { return Math.min(maxStack, item.getType().getMaxStackSize() > 0 ? item.getType().getMaxStackSize() : 64); }
    /** Bukkit addItem: tops up similar stacks first, then fills empty slots; returns what did not fit by argument index. */
    private java.util.HashMap<Integer, org.bukkit.inventory.ItemStack> addItems(org.bukkit.inventory.ItemStack[] items) {
        var left = new java.util.HashMap<Integer, org.bukkit.inventory.ItemStack>();
        for (int i = 0; i < items.length; i++) {
            var item = items[i];
            if (empty(item)) continue;
            int remaining = item.getAmount();
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                var present = contents[slot];
                if (empty(present) || !alike(present, item)) continue;
                int room = limit(item) - present.getAmount();
                if (room <= 0) continue;
                int moved = Math.min(room, remaining);
                present.setAmount(present.getAmount() + moved);
                remaining -= moved;
            }
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                if (!empty(contents[slot])) continue;
                int moved = Math.min(limit(item), remaining);
                var placed = item.clone();
                placed.setAmount(moved);
                contents[slot] = placed;
                remaining -= moved;
            }
            if (remaining > 0) { var rest = item.clone(); rest.setAmount(remaining); left.put(i, rest); }
        }
        return left;
    }
    /** Bukkit removeItem: removes similar items, taking from the first slots; returns what could not be removed. */
    private java.util.HashMap<Integer, org.bukkit.inventory.ItemStack> removeItems(org.bukkit.inventory.ItemStack[] items, boolean exactAmountsOnly) {
        var left = new java.util.HashMap<Integer, org.bukkit.inventory.ItemStack>();
        for (int i = 0; i < items.length; i++) {
            var item = items[i];
            if (empty(item)) continue;
            int remaining = item.getAmount();
            for (int slot = 0; slot < contents.length && remaining > 0; slot++) {
                var present = contents[slot];
                if (empty(present) || !alike(present, item)) continue;
                if (present.getAmount() <= remaining) { remaining -= present.getAmount(); contents[slot] = null; }
                else { present.setAmount(present.getAmount() - remaining); remaining = 0; }
            }
            if (remaining > 0) { var rest = item.clone(); rest.setAmount(remaining); left.put(i, rest); }
        }
        return left;
    }
    private int total(Object wanted, boolean similar) {
        int sum = 0;
        for (var slot : contents) if (matches(slot, wanted, similar)) sum += slot.getAmount();
        return sum;
    }
    private boolean contains(Object[] args) {
        Object wanted = args[0];
        if (wanted == null) return false;
        if (args.length == 1) {
            for (var slot : contents) if (matches(slot, wanted, false)) return true;
            return false;
        }
        return total(wanted, true) >= (int) args[1];
    }
    private int first(Object wanted) {
        if (wanted instanceof org.bukkit.Material) { for (int i = 0; i < contents.length; i++) if (matches(contents[i], wanted, true)) return i; return -1; }
        for (int i = 0; i < contents.length; i++) if (matches(contents[i], wanted, false)) return i;
        return -1;
    }
    private java.util.HashMap<Integer, org.bukkit.inventory.ItemStack> all(Object wanted) {
        var found = new java.util.HashMap<Integer, org.bukkit.inventory.ItemStack>();
        for (int i = 0; i < contents.length; i++) if (matches(contents[i], wanted, false)) found.put(i, contents[i]);
        return found;
    }
    private void removeAll(Object wanted) { for (int i = 0; i < contents.length; i++) if (matches(contents[i], wanted, false)) contents[i] = null; }
    @Override public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getSize" -> contents.length;
            case "getMaxStackSize" -> maxStack;
            case "setMaxStackSize" -> { maxStack = (int) args[0]; yield null; }
            case "getHolder" -> holder;
            case "getType" -> type;
            case "getItem" -> contents[(int) args[0]];
            case "setItem" -> { contents[(int) args[0]] = (org.bukkit.inventory.ItemStack) args[1]; yield null; }
            case "getContents", "getStorageContents" -> contents.clone();
            case "setContents", "setStorageContents" -> { var items = (org.bukkit.inventory.ItemStack[]) args[0]; if (items.length > contents.length) throw new IllegalArgumentException("Too many items"); java.util.Arrays.fill(contents, null); System.arraycopy(items, 0, contents, 0, items.length); yield null; }
            case "clear" -> { if (args == null || args.length == 0) java.util.Arrays.fill(contents, null); else contents[(int) args[0]] = null; yield null; }
            case "firstEmpty" -> { int index = -1; for (int i = 0; i < contents.length; i++) if (contents[i] == null || isAir(contents[i].getType())) { index = i; break; } yield index; }
            case "isEmpty" -> java.util.Arrays.stream(contents).allMatch(item -> item == null || isAir(item.getType()));
            case "getViewers" -> java.util.List.of();
            case "iterator" -> java.util.Arrays.asList(contents).listIterator(args == null || args.length == 0 ? 0 : (int) args[0]);
            case "addItem" -> addItems((org.bukkit.inventory.ItemStack[]) args[0]);
            case "removeItem" -> removeItems((org.bukkit.inventory.ItemStack[]) args[0], true);
            case "removeItemAnySlot" -> removeItems((org.bukkit.inventory.ItemStack[]) args[0], false);
            case "contains" -> contains(args);
            case "containsAtLeast" -> total(args[0], true) >= (int) args[1];
            case "first" -> first(args[0]);
            case "all" -> all(args[0]);
            case "remove" -> { removeAll(args[0]); yield null; }
            case "getLocation" -> null;
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> "FerriteInventory[" + type + ", size=" + contents.length + "]";
            default -> HostBridge.dispatch("Inventory", "", method, args);
        };
    }
}
