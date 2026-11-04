package dev.ferrite.server;

/** In-memory inventory for plugin-created containers, independent of world inventories. */
final class FerriteInventory implements java.lang.reflect.InvocationHandler {
    private final org.bukkit.inventory.InventoryHolder holder;
    private final org.bukkit.event.inventory.InventoryType type;
    private final org.bukkit.inventory.ItemStack[] contents;
    private final org.bukkit.inventory.Inventory proxy;
    private int maxStack = 64;

    private FerriteInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, int size) {
        this.holder = holder; this.type = type; this.contents = new org.bukkit.inventory.ItemStack[size];
        this.proxy = (org.bukkit.inventory.Inventory) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.Inventory.class}, this);
    }
    static org.bukkit.inventory.Inventory create(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, int size) {
        if (size < 1 || (type == org.bukkit.event.inventory.InventoryType.CHEST && size % 9 != 0)) throw new IllegalArgumentException("Invalid inventory size: " + size);
        return new FerriteInventory(holder, type, size).proxy;
    }
    private static boolean isAir(org.bukkit.Material material) { return material == org.bukkit.Material.AIR || material == org.bukkit.Material.CAVE_AIR || material == org.bukkit.Material.VOID_AIR; }
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
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> "FerriteInventory[" + type + ", size=" + contents.length + "]";
            default -> HostBridge.dispatch("Inventory", "", method, args);
        };
    }
}
