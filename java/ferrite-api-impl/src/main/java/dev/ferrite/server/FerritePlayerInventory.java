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
        var wire = ItemMetaWire.of(item);
        int damage = wire.damage();
        Integer mapId = wire.mapId();
        String meta = wire.meta();
        String material = item == null ? "" : item.getType().getKey().toString();
        int amount = item == null ? 0 : item.getAmount();
        if (!Native.playerInventory(player.getUniqueId().toString(), slot, material, amount)) throw new IllegalStateException("No player command handler");
        if (damage > 0) HostBridge.callVoid("Player", player.getUniqueId().toString(), "setItemDamage(int,int)", slot, damage);
        if (meta != null) HostBridge.callVoid("Player", player.getUniqueId().toString(), "setItemMeta(int,String)", slot, meta);
        if (mapId != null) HostBridge.callVoid("Player", player.getUniqueId().toString(), "setItemMapId(int,int)", slot, mapId);
        items[slot] = item == null ? null : item.clone();
    }
    /** An inventory index, or the index an equipment slot lives at (0 main hand, 36 boots .. 39 helmet, 40 off hand). */
    private static int slotOf(Object slot) {
        if (slot instanceof org.bukkit.inventory.EquipmentSlot equipment) {
            return switch (equipment) {
                case HAND -> 0;
                case OFF_HAND -> 40;
                case FEET -> 36;
                case LEGS -> 37;
                case CHEST -> 38;
                case HEAD -> 39;
                // Player#canUseSlot is false for exactly these two in vanilla: they exist for animals' armor and saddles.
                case BODY, SADDLE -> throw new UnsupportedOperationException("Ferrite: a player has no " + equipment + " equipment slot (vanilla Player#canUseSlot is false for BODY and SADDLE)");
            };
        }
        return (int) slot;
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getSize" -> items.length;
            case "getHolder" -> player;
            case "getType" -> org.bukkit.event.inventory.InventoryType.PLAYER;
            case "getItem" -> read(slotOf(args[0]));
            case "setItem" -> { write(slotOf(args[0]), (org.bukkit.inventory.ItemStack) args[1]); yield null; }
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
