package net.minecraft.world.inventory;

/**
 * Menu identities plugins name when they send an open-screen packet. Ferrite can currently
 * present the generic chest family; the other constants exist so plugin classes load, and the
 * connection rejects them with a named exception instead of pretending the screen opened.
 */
public final class MenuType<T> {
    private final String name;
    private final int chestRows;

    private MenuType(String name, int chestRows) {
        this.name = name;
        this.chestRows = chestRows;
    }

    public String getName() { return name; }
    /** Number of chest rows this menu represents, or 0 when Ferrite cannot present it. */
    public int getChestRows() { return chestRows; }
    @Override public String toString() { return name; }

    public static final MenuType<Object> GENERIC_9x1 = new MenuType<>("GENERIC_9x1", 1);
    public static final MenuType<Object> GENERIC_9x2 = new MenuType<>("GENERIC_9x2", 2);
    public static final MenuType<Object> GENERIC_9x3 = new MenuType<>("GENERIC_9x3", 3);
    public static final MenuType<Object> GENERIC_9x4 = new MenuType<>("GENERIC_9x4", 4);
    public static final MenuType<Object> GENERIC_9x5 = new MenuType<>("GENERIC_9x5", 5);
    public static final MenuType<Object> GENERIC_9x6 = new MenuType<>("GENERIC_9x6", 6);
    public static final MenuType<Object> GENERIC_3x3 = new MenuType<>("GENERIC_3x3", 0);
    public static final MenuType<Object> CRAFTER_3x3 = new MenuType<>("CRAFTER_3x3", 0);
    public static final MenuType<Object> ANVIL = new MenuType<>("ANVIL", 0);
    public static final MenuType<Object> BEACON = new MenuType<>("BEACON", 0);
    public static final MenuType<Object> BLAST_FURNACE = new MenuType<>("BLAST_FURNACE", 0);
    public static final MenuType<Object> BREWING_STAND = new MenuType<>("BREWING_STAND", 0);
    public static final MenuType<Object> CRAFTING = new MenuType<>("CRAFTING", 0);
    public static final MenuType<Object> ENCHANTMENT = new MenuType<>("ENCHANTMENT", 0);
    public static final MenuType<Object> FURNACE = new MenuType<>("FURNACE", 0);
    public static final MenuType<Object> GRINDSTONE = new MenuType<>("GRINDSTONE", 0);
    public static final MenuType<Object> HOPPER = new MenuType<>("HOPPER", 0);
    public static final MenuType<Object> LECTERN = new MenuType<>("LECTERN", 0);
    public static final MenuType<Object> LOOM = new MenuType<>("LOOM", 0);
    public static final MenuType<Object> MERCHANT = new MenuType<>("MERCHANT", 0);
    public static final MenuType<Object> SHULKER_BOX = new MenuType<>("SHULKER_BOX", 0);
    public static final MenuType<Object> SMITHING = new MenuType<>("SMITHING", 0);
    public static final MenuType<Object> SMOKER = new MenuType<>("SMOKER", 0);
    public static final MenuType<Object> CARTOGRAPHY_TABLE = new MenuType<>("CARTOGRAPHY_TABLE", 0);
    public static final MenuType<Object> STONECUTTER = new MenuType<>("STONECUTTER", 0);
}
