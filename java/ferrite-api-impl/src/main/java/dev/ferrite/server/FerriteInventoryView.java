package dev.ferrite.server;

/** The view of an in-memory inventory a plugin opened for itself. */
final class FerriteInventoryView implements org.bukkit.inventory.InventoryView {
    private final org.bukkit.inventory.Inventory top;
    private final org.bukkit.inventory.Inventory bottom;
    private final org.bukkit.entity.HumanEntity player;
    private final org.bukkit.event.inventory.InventoryType type;
    private org.bukkit.inventory.ItemStack cursor;
    private String title = "Ferrite";

    private FerriteInventoryView(org.bukkit.inventory.Inventory top, org.bukkit.inventory.Inventory bottom, org.bukkit.entity.HumanEntity player, org.bukkit.event.inventory.InventoryType type) {
        this.top = top;
        this.bottom = bottom;
        this.player = player;
        this.type = type;
    }

    static FerriteInventoryView create(org.bukkit.entity.HumanEntity player, org.bukkit.inventory.Inventory top) {
        return new FerriteInventoryView(top, player.getInventory(), player, top.getType());
    }

    @Override public org.bukkit.inventory.Inventory getTopInventory() { return top; }
    @Override public org.bukkit.inventory.Inventory getBottomInventory() { return bottom; }
    @Override public org.bukkit.entity.HumanEntity getPlayer() { return player; }
    @Override public org.bukkit.event.inventory.InventoryType getType() { return type; }
    @Override public void setItem(int slot, org.bukkit.inventory.ItemStack item) { inventory(slot).setItem(relative(slot), item); }
    @Override public org.bukkit.inventory.ItemStack getItem(int slot) { return inventory(slot).getItem(relative(slot)); }
    @Override public void setCursor(org.bukkit.inventory.ItemStack item) { cursor = item; }
    @Override public org.bukkit.inventory.ItemStack getCursor() { return cursor; }
    @Override public org.bukkit.inventory.Inventory getInventory(int rawSlot) { return rawSlot < top.getSize() ? top : bottom; }
    @Override public int convertSlot(int rawSlot) { return relative(rawSlot); }
    @Override public org.bukkit.event.inventory.InventoryType.SlotType getSlotType(int rawSlot) {
        return rawSlot < top.getSize() ? org.bukkit.event.inventory.InventoryType.SlotType.CONTAINER : org.bukkit.event.inventory.InventoryType.SlotType.QUICKBAR;
    }
    @Override public void open() {}
    @Override public void close() {}
    @Override public int countSlots() { return top.getSize() + bottom.getSize(); }
    @Override public boolean setProperty(org.bukkit.inventory.InventoryView.Property property, int value) { return false; }
    @Override public String getTitle() { return title; }
    @Override public String getOriginalTitle() { return title; }
    @Override public void setTitle(String value) { title = java.util.Objects.requireNonNull(value, "title"); }
    /** The vanilla menu type the client shows for the top inventory (the `minecraft:menu` registry). */
    @Override public org.bukkit.inventory.MenuType getMenuType() {
        return switch (top.getType()) {
            case CHEST -> switch (top.getSize() / 9) {
                case 1 -> org.bukkit.inventory.MenuType.GENERIC_9X1;
                case 2 -> org.bukkit.inventory.MenuType.GENERIC_9X2;
                case 3 -> org.bukkit.inventory.MenuType.GENERIC_9X3;
                case 4 -> org.bukkit.inventory.MenuType.GENERIC_9X4;
                case 5 -> org.bukkit.inventory.MenuType.GENERIC_9X5;
                case 6 -> org.bukkit.inventory.MenuType.GENERIC_9X6;
                default -> throw new UnsupportedOperationException("Ferrite: a chest inventory of " + top.getSize() + " slots has no menu type");
            };
            case ENDER_CHEST, BARREL -> org.bukkit.inventory.MenuType.GENERIC_9X3;
            case SHULKER_BOX -> org.bukkit.inventory.MenuType.SHULKER_BOX;
            case ANVIL -> org.bukkit.inventory.MenuType.ANVIL;
            case CARTOGRAPHY -> org.bukkit.inventory.MenuType.CARTOGRAPHY_TABLE;
            default -> throw new UnsupportedOperationException("Ferrite: no menu type is mapped for inventory type " + top.getType());
        };
    }
    private org.bukkit.inventory.Inventory inventory(int rawSlot) { return rawSlot < top.getSize() ? top : bottom; }
    private int relative(int rawSlot) { return rawSlot < top.getSize() ? rawSlot : rawSlot - top.getSize(); }
}
