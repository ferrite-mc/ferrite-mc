package dev.ferrite.server;

import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** `Player#getEquipment` covers the whole `EntityEquipment` interface on the player's inventory. */
class FerriteEntityEquipmentTest {
    private static final java.util.UUID WORLD_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000c1");
    private static final java.util.UUID PLAYER_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000c2");
    private static FerriteServer server;
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) Bukkit.getServer();
        var world = FerriteWorld.create(server, WORLD_ID, "equipment_world", 0L, 0, 64, 0);
        server.registerWorld(world);
        player = FerritePlayer.create(server, PLAYER_ID, "Dresser", false, 20.0, 20, 0, new org.bukkit.Location(world, 0.5, 65.0, 0.5));
        server.registerPlayer(player);
    }

    @AfterAll static void stop() {
        server.unregisterWorld(WORLD_ID);
        server.unregisterPlayer(PLAYER_ID);
    }

    @Test void everyArmorAndHandSlotRoundTripsIncludingTheSilentOverloads() {
        var equipment = player.getEquipment();
        assertSame(equipment, player.getEquipment());
        assertSame(player, equipment.getHolder());
        equipment.setHelmet(new ItemStack(Material.IRON_HELMET));
        equipment.setChestplate(new ItemStack(Material.IRON_CHESTPLATE), true);
        equipment.setLeggings(new ItemStack(Material.IRON_LEGGINGS), true);
        equipment.setBoots(new ItemStack(Material.IRON_BOOTS), true);
        equipment.setItemInMainHand(new ItemStack(Material.DIAMOND_SWORD), true);
        equipment.setItemInOffHand(new ItemStack(Material.SHIELD), true);
        assertEquals(Material.IRON_HELMET, equipment.getHelmet().getType());
        assertEquals(Material.IRON_CHESTPLATE, equipment.getItem(EquipmentSlot.CHEST).getType());
        assertEquals(Material.IRON_LEGGINGS, equipment.getLeggings().getType());
        assertEquals(Material.IRON_BOOTS, equipment.getBoots().getType());
        assertEquals(Material.DIAMOND_SWORD, equipment.getItemInHand().getType());
        assertEquals(Material.SHIELD, equipment.getItemInOffHand().getType());
        var armor = equipment.getArmorContents();
        assertEquals(4, armor.length);
        assertEquals(Material.IRON_BOOTS, armor[0].getType());
        assertEquals(Material.IRON_HELMET, armor[3].getType());
    }

    @Test void clearEmptiesOnlyTheEquipmentNotTheRestOfTheInventory() {
        var equipment = player.getEquipment();
        player.getInventory().setItem(20, new ItemStack(Material.APPLE, 5));
        equipment.setHelmet(new ItemStack(Material.GOLDEN_HELMET));
        equipment.setItemInMainHand(new ItemStack(Material.STICK));
        equipment.clear();
        assertTrue(equipment.getHelmet() == null || equipment.getHelmet().getType().isAir());
        assertTrue(equipment.getItemInMainHand().getType().isAir());
        assertEquals(Material.APPLE, player.getInventory().getItem(20).getType(), "the backpack stays");
    }

    @Test void aPlayerHasNoBodyOrSaddleAndNoSettableDropChance() {
        var equipment = player.getEquipment();
        assertTrue(equipment.getItem(EquipmentSlot.BODY).getType().isAir());
        assertTrue(equipment.getItem(EquipmentSlot.SADDLE).getType().isAir());
        equipment.setItem(EquipmentSlot.BODY, new ItemStack(Material.AIR));
        assertThrows(IllegalArgumentException.class, () -> equipment.setItem(EquipmentSlot.SADDLE, new ItemStack(Material.SADDLE)));
        assertEquals(1.0f, equipment.getHelmetDropChance());
        assertEquals(1.0f, equipment.getDropChance(EquipmentSlot.HAND));
        assertThrows(UnsupportedOperationException.class, () -> equipment.setBootsDropChance(0f));
        assertThrows(UnsupportedOperationException.class, () -> equipment.setDropChance(EquipmentSlot.CHEST, 0.5f));
        assertThrows(NullPointerException.class, () -> equipment.getItem(null));
    }
}
