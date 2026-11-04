package dev.ferrite.server;

final class FerritePlayerInventory implements java.lang.reflect.InvocationHandler {
    private final FerritePlayer player;
    private final org.bukkit.inventory.ItemStack[] items = new org.bukkit.inventory.ItemStack[41];
    final org.bukkit.inventory.PlayerInventory proxy;
    FerritePlayerInventory(FerritePlayer player) {
        this.player = player;
        proxy = (org.bukkit.inventory.PlayerInventory) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.PlayerInventory.class}, this);
    }
    void setSnapshot(int slot, String material, int amount) {
        var type = java.util.Objects.requireNonNull(org.bukkit.Material.matchMaterial(material), "unknown material");
        items[slot] = new org.bukkit.inventory.ItemStack(type, amount);
    }
    private org.bukkit.inventory.ItemStack read(int slot) { var item = items[slot]; return item == null ? null : item.clone(); }
    private void write(int slot, org.bukkit.inventory.ItemStack item) {
        if (item != null && item.hasItemMeta()) throw new UnsupportedOperationException("Ferrite: inventory metadata commands are not implemented");
        String material = item == null ? "" : item.getType().getKey().toString();
        int amount = item == null ? 0 : item.getAmount();
        if (!Native.playerInventory(player.getUniqueId().toString(), slot, material, amount)) throw new IllegalStateException("No player command handler");
        items[slot] = item == null ? null : item.clone();
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getSize" -> items.length;
            case "getHolder" -> player;
            case "getType" -> org.bukkit.event.inventory.InventoryType.PLAYER;
            case "getItem" -> read((int) args[0]);
            case "setItem" -> { write((int) args[0], (org.bukkit.inventory.ItemStack) args[1]); yield null; }
            case "getItemInMainHand" -> read(0);
            case "setItemInMainHand" -> { write(0, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getItemInOffHand" -> read(40);
            case "setItemInOffHand" -> { write(40, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getHelmet" -> read(39); case "getChestplate" -> read(38); case "getLeggings" -> read(37); case "getBoots" -> read(36);
            case "setHelmet" -> { write(39, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setChestplate" -> { write(38, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setLeggings" -> { write(37, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setBoots" -> { write(36, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getHeldItemSlot" -> 0;
            case "getContents" -> { var copy = new org.bukkit.inventory.ItemStack[items.length]; for (int i = 0; i < items.length; i++) copy[i] = read(i); yield copy; }
            case "getStorageContents" -> { var copy = new org.bukkit.inventory.ItemStack[36]; for (int i = 0; i < copy.length; i++) copy[i] = read(i); yield copy; }
            case "getViewers" -> java.util.List.of(player);
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> "PlayerInventory[" + player.getUniqueId() + "]";
            default -> HostBridge.dispatch("PlayerInventory", "", method, args);
        };
    }
}
