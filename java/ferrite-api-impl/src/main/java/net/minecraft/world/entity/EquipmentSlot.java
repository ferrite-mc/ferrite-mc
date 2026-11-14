package net.minecraft.world.entity;

/** NMS equipment slots, mapped to the Bukkit slot each one is the same hand/armour piece as. */
public enum EquipmentSlot {
    MAINHAND(org.bukkit.inventory.EquipmentSlot.HAND),
    OFFHAND(org.bukkit.inventory.EquipmentSlot.OFF_HAND),
    FEET(org.bukkit.inventory.EquipmentSlot.FEET),
    LEGS(org.bukkit.inventory.EquipmentSlot.LEGS),
    CHEST(org.bukkit.inventory.EquipmentSlot.CHEST),
    HEAD(org.bukkit.inventory.EquipmentSlot.HEAD),
    BODY(org.bukkit.inventory.EquipmentSlot.BODY);

    private final org.bukkit.inventory.EquipmentSlot bukkit;

    EquipmentSlot(org.bukkit.inventory.EquipmentSlot bukkit) { this.bukkit = bukkit; }

    public org.bukkit.inventory.EquipmentSlot bukkit() { return bukkit; }

    public static EquipmentSlot of(org.bukkit.inventory.EquipmentSlot slot) {
        for (var value : values()) if (value.bukkit == slot) return value;
        throw new IllegalArgumentException("No NMS equipment slot for " + slot);
    }
}
